use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use anyhow::{Context, Result, anyhow};
use hound::{SampleFormat as WavSampleFormat, WavSpec, WavWriter};

use super::{
    FinishedRecording, JOURNAL_EXTENSION, RecoverableRecording, TARGET_SAMPLE_RATE,
    WAV_HEADER_BYTES, WAV_SAMPLE_BYTES,
    journal::{discard_marker_path_for_recording, read_journal, write_journal_state},
};
pub(super) fn wav_data_sample_count(path: &Path) -> Result<u64> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to stat WAV source {}", path.display()))?;
    if metadata.len() < WAV_HEADER_BYTES {
        return Err(anyhow!("WAV source is shorter than its header"));
    }

    let mut file = File::open(path)
        .with_context(|| format!("failed to open WAV source {}", path.display()))?;
    let mut header = [0_u8; WAV_HEADER_BYTES as usize];
    file.read_exact(&mut header)
        .with_context(|| format!("failed to read WAV header {}", path.display()))?;
    if &header[0..4] != b"RIFF"
        || &header[8..12] != b"WAVE"
        || &header[12..16] != b"fmt "
        || &header[36..40] != b"data"
        || u32::from_le_bytes(header[16..20].try_into().unwrap()) != 16
        || u16::from_le_bytes(header[20..22].try_into().unwrap()) != 1
        || u16::from_le_bytes(header[22..24].try_into().unwrap()) != 1
        || u32::from_le_bytes(header[24..28].try_into().unwrap()) != TARGET_SAMPLE_RATE
        || u16::from_le_bytes(header[34..36].try_into().unwrap()) != 16
    {
        return Err(anyhow!("WAV source has an unsupported or corrupt header"));
    }

    // An interrupted header update can advertise fewer bytes than a previously
    // synced journal checkpoint. The journal bounds the durable prefix; use
    // physical PCM extent here so a torn size field cannot discard that prefix.
    Ok(metadata.len().saturating_sub(WAV_HEADER_BYTES) / WAV_SAMPLE_BYTES)
}

pub(super) fn list_recoverable_recordings(
    recordings_dir: &Path,
) -> Result<Vec<RecoverableRecording>> {
    if !recordings_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut recoverable = Vec::new();
    for entry in fs::read_dir(recordings_dir).with_context(|| {
        format!(
            "failed to inspect recoverable recordings directory {}",
            recordings_dir.display()
        )
    })? {
        let Ok(entry) = entry else {
            // A single disappearing or inaccessible directory entry must not
            // hide the other recoverable captures.
            continue;
        };
        let journal_path = entry.path();
        if journal_path.extension().and_then(|value| value.to_str()) != Some(JOURNAL_EXTENSION) {
            continue;
        }
        let Some(id) = journal_path
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let Some(journal) = read_journal(&journal_path) else {
            // Fail closed on a damaged journal and retain both files for
            // operator inspection or a later repair.
            continue;
        };
        if journal.id != id || journal.sample_count == 0 {
            continue;
        }

        let path = journal_path.with_extension("wav");
        if journal.state == "discarded" || discard_marker_path_for_recording(&path).is_file() {
            continue;
        }
        let Ok(file_sample_count) = wav_data_sample_count(&path) else {
            continue;
        };
        let sample_count = file_sample_count.min(journal.sample_count);
        if sample_count == 0 {
            continue;
        }
        let duration_ms = sample_count
            .saturating_mul(1_000)
            .checked_div(TARGET_SAMPLE_RATE as u64)
            .unwrap_or(0);
        recoverable.push(RecoverableRecording {
            id: id.to_string(),
            path,
            journal_path,
            sample_rate: TARGET_SAMPLE_RATE,
            sample_count,
            duration_ms,
            materialized: matches!(journal.state.as_str(), "complete" | "recovered"),
        });
    }
    recoverable.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(recoverable)
}

pub(super) fn recover_recording(recordings_dir: &Path, id: &str) -> Result<FinishedRecording> {
    if id.is_empty() || id.contains(['/', '\\']) {
        return Err(anyhow!("invalid recoverable recording id"));
    }
    let recordings = list_recoverable_recordings(recordings_dir)?;
    let recording = recordings
        .into_iter()
        .find(|recording| recording.id == id)
        .ok_or_else(|| anyhow!("recoverable recording not found: {id}"))?;

    if !recording.materialized || !wav_header_matches_data(&recording.path) {
        recover_wav_from_partial(&recording.path, &recording.path, recording.sample_count)
            .with_context(|| format!("failed to recover recording {}", recording.id))?;
        write_journal_state(
            &recording.journal_path,
            &recording.id,
            recording.sample_count,
            "recovered",
        )
        .with_context(|| format!("failed to mark recording {} recovered", recording.id))?;
    } else if wav_data_sample_count(&recording.path).unwrap_or(0) == 0 {
        return Err(anyhow!(
            "materialized recording {} has no valid WAV samples",
            recording.id
        ));
    }

    Ok(FinishedRecording {
        id: recording.id,
        path: recording.path,
        duration_ms: recording.duration_ms,
    })
}

pub(super) fn acknowledge_recovered_recording(recordings_dir: &Path, id: &str) -> Result<()> {
    if id.is_empty() || id.contains(['/', '\\']) {
        return Err(anyhow!("invalid recoverable recording id"));
    }
    let recording = list_recoverable_recordings(recordings_dir)?
        .into_iter()
        .find(|recording| recording.id == id)
        .ok_or_else(|| anyhow!("recoverable recording not found: {id}"))?;
    if !recording.materialized || !wav_header_matches_data(&recording.path) {
        return Err(anyhow!(
            "recording {id} must be materialized before acknowledgement"
        ));
    }
    fs::remove_file(&recording.journal_path).with_context(|| {
        format!(
            "failed to acknowledge recovered recording {}",
            recording.journal_path.display()
        )
    })?;
    Ok(())
}

pub(super) fn recover_wav_from_partial(
    source_path: &Path,
    target_path: &Path,
    sample_count: u64,
) -> Result<()> {
    let available_samples = wav_data_sample_count(source_path)?;
    let samples_to_copy = available_samples.min(sample_count);
    if samples_to_copy == 0 {
        return Err(anyhow!("WAV source has no durable samples"));
    }

    let mut input = File::open(source_path)
        .with_context(|| format!("failed to open {}", source_path.display()))?;
    input.seek(SeekFrom::Start(WAV_HEADER_BYTES))?;
    let temporary_path = target_path.with_extension("wav.recovering");
    let spec = WavSpec {
        channels: 1,
        sample_rate: TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: WavSampleFormat::Int,
    };
    let mut writer = WavWriter::create(&temporary_path, spec)
        .with_context(|| format!("failed to create {}", temporary_path.display()))?;
    let mut bytes_remaining = samples_to_copy
        .checked_mul(WAV_SAMPLE_BYTES)
        .ok_or_else(|| anyhow!("recovery sample count is too large"))?;
    let mut bytes = [0_u8; 64 * 1024];
    while bytes_remaining > 0 {
        let read_len = bytes_remaining.min(bytes.len() as u64) as usize;
        input
            .read_exact(&mut bytes[..read_len])
            .context("WAV source ended before the durable prefix")?;
        for sample in bytes[..read_len].chunks_exact(WAV_SAMPLE_BYTES as usize) {
            writer.write_sample(i16::from_le_bytes([sample[0], sample[1]]))?;
        }
        bytes_remaining -= read_len as u64;
    }
    writer
        .finalize()
        .context("failed to finalize recovered WAV")?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(&temporary_path)
        .and_then(|file| file.sync_all())
        .context("failed to flush recovered WAV")?;
    drop(input);
    atomic_replace(&temporary_path, target_path)
        .with_context(|| format!("failed to publish recovered WAV {}", target_path.display()))?;
    Ok(())
}

fn atomic_replace(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::{
            Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            },
            core::PCWSTR,
        };

        let source_wide = source
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let destination_wide = destination
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();

        unsafe {
            MoveFileExW(
                PCWSTR(source_wide.as_ptr()),
                PCWSTR(destination_wide.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
            .map_err(Into::into)
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        fs::rename(source, destination).map_err(Into::into)
    }
}

fn wav_header_matches_data(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let Ok(metadata) = file.metadata() else {
        return false;
    };
    let mut header = [0_u8; WAV_HEADER_BYTES as usize];
    if file.read_exact(&mut header).is_err() {
        return false;
    }
    let declared = u32::from_le_bytes(header[40..44].try_into().unwrap()) as u64;
    declared == metadata.len().saturating_sub(WAV_HEADER_BYTES)
}
