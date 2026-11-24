use std::{
    fs::{self, File, OpenOptions},
    io::{self, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};

use super::{DISCARD_MARKER_EXTENSION, JOURNAL_EXTENSION, TARGET_SAMPLE_RATE};
pub(super) fn journal_path_for_recording(path: &Path) -> PathBuf {
    path.with_extension(JOURNAL_EXTENSION)
}

pub(super) fn discard_marker_path_for_recording(path: &Path) -> PathBuf {
    path.with_extension(DISCARD_MARKER_EXTENSION)
}

pub(super) fn write_discard_marker(path: &Path, id: &str) -> Result<()> {
    let marker_path = discard_marker_path_for_recording(path);
    let mut marker = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&marker_path)
        .with_context(|| format!("failed to create discard marker {}", marker_path.display()))?;
    marker.write_all(format!("version=1\nid={id}\nstate=discarded\n").as_bytes())?;
    marker.flush()?;
    marker.sync_data()?;
    Ok(())
}

pub(super) fn clear_discard_marker(path: &Path) {
    let _ = fs::remove_file(discard_marker_path_for_recording(path));
}

#[derive(Debug)]
pub(super) struct JournalMetadata {
    pub(super) id: String,
    pub(super) state: String,
    pub(super) sample_count: u64,
    pub(super) sequence: u64,
    pub(super) valid_bytes: u64,
}

fn next_complete_journal_line<'a>(contents: &'a str, offset: &mut usize) -> Option<&'a str> {
    let remaining = &contents[*offset..];
    let newline = remaining.find('\n')?;
    let line = &remaining[..newline];
    *offset += newline + 1;
    Some(line.strip_suffix('\r').unwrap_or(line))
}

fn journal_checksum(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Read the last complete, checksummed journal record. A torn append is ignored,
/// leaving the previous synced record as the recovery point.
pub(super) fn read_journal(path: &Path) -> Option<JournalMetadata> {
    let contents = fs::read_to_string(path).ok()?;
    let mut offset = 0_usize;
    if next_complete_journal_line(&contents, &mut offset)? != "version=2" {
        return None;
    }
    let id = next_complete_journal_line(&contents, &mut offset)?
        .strip_prefix("id=")
        .filter(|value| !value.is_empty())?
        .to_string();
    if next_complete_journal_line(&contents, &mut offset)? != "format=wav_pcm_s16le" {
        return None;
    }
    if next_complete_journal_line(&contents, &mut offset)?
        != format!("sample_rate={TARGET_SAMPLE_RATE}")
    {
        return None;
    }
    if next_complete_journal_line(&contents, &mut offset)? != "journal=append_records_v1" {
        return None;
    }

    let mut valid_bytes = offset;
    let mut latest: Option<(u64, String, u64)> = None;
    loop {
        let Some(line) = next_complete_journal_line(&contents, &mut offset) else {
            break;
        };
        if line.is_empty() {
            valid_bytes = offset;
            continue;
        }
        let Some(payload) = line.strip_prefix("record=") else {
            break;
        };
        let Some((body_with_length, checksum_text)) = payload.rsplit_once('|') else {
            break;
        };
        let Some((body, length_text)) = body_with_length.rsplit_once("|len=") else {
            break;
        };
        let Ok(declared_length) = length_text.parse::<usize>() else {
            break;
        };
        if declared_length != "record=".len() + body.len() {
            break;
        }
        let Ok(checksum) = u64::from_str_radix(checksum_text, 16) else {
            break;
        };
        let canonical = format!("record={body}");
        if journal_checksum(canonical.as_bytes()) != checksum {
            break;
        }

        let mut fields = body.split('|');
        let Some(sequence) = fields.next().and_then(|value| value.parse::<u64>().ok()) else {
            break;
        };
        let Some(state) = fields
            .next()
            .filter(|value| matches!(*value, "active" | "complete" | "recovered" | "discarded"))
        else {
            break;
        };
        let Some(sample_count) = fields.next().and_then(|value| value.parse::<u64>().ok()) else {
            break;
        };
        if fields.next().is_some()
            || latest
                .as_ref()
                .is_some_and(|(previous, _, _)| sequence <= *previous)
        {
            break;
        }
        latest = Some((sequence, state.to_string(), sample_count));
        valid_bytes = offset;
    }

    let (sequence, state, sample_count) = latest?;
    Some(JournalMetadata {
        id,
        state,
        sample_count,
        sequence,
        valid_bytes: valid_bytes as u64,
    })
}

pub(super) fn write_journal_state(
    path: &Path,
    id: &str,
    sample_count: u64,
    state: &str,
) -> Result<()> {
    if !matches!(state, "active" | "complete" | "recovered" | "discarded") {
        return Err(anyhow!("invalid capture journal state"));
    }
    let metadata = read_journal(path).ok_or_else(|| {
        anyhow!(
            "capture journal is corrupt or has no complete checkpoint: {}",
            path.display()
        )
    })?;
    if metadata.id != id {
        return Err(anyhow!("capture journal id does not match recording"));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .open(path)
        .with_context(|| format!("failed to open capture journal {}", path.display()))?;
    file.set_len(metadata.valid_bytes)?;
    file.seek(SeekFrom::Start(metadata.valid_bytes))?;
    let mut sequence = metadata
        .sequence
        .checked_add(1)
        .ok_or_else(|| anyhow!("capture journal sequence exhausted"))?;
    append_journal_record(&mut file, &mut sequence, state, sample_count)?;
    Ok(())
}

pub(super) struct CaptureJournal {
    file: File,
    next_sequence: u64,
    sample_count: u64,
}

fn append_journal_record(
    file: &mut File,
    next_sequence: &mut u64,
    state: &str,
    sample_count: u64,
) -> io::Result<()> {
    if !matches!(state, "active" | "complete" | "recovered" | "discarded") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid capture journal state",
        ));
    }
    if *next_sequence == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "capture journal sequence exhausted",
        ));
    }
    let payload = format!("record={}|{}|{}", *next_sequence, state, sample_count);
    let checksum = journal_checksum(payload.as_bytes());
    let record = format!("{payload}|len={}|{checksum:016x}\n", payload.len());
    file.seek(SeekFrom::End(0))?;
    file.write_all(record.as_bytes())?;
    file.flush()?;
    file.sync_data()?;
    *next_sequence = next_sequence.saturating_add(1);
    Ok(())
}

impl CaptureJournal {
    pub(super) fn create(path: &Path, id: &str) -> Result<Self> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
            .with_context(|| format!("failed to create capture journal {}", path.display()))?;
        file.write_all(
            format!(
                "version=2\nid={id}\nformat=wav_pcm_s16le\nsample_rate={TARGET_SAMPLE_RATE}\njournal=append_records_v1\n"
            )
            .as_bytes(),
        )?;
        file.flush()?;
        file.sync_data()?;
        let mut journal = Self {
            file,
            next_sequence: 1,
            sample_count: 0,
        };
        journal.append_record("active", 0)?;
        Ok(journal)
    }

    fn append_record(&mut self, state: &str, sample_count: u64) -> io::Result<()> {
        append_journal_record(&mut self.file, &mut self.next_sequence, state, sample_count)
    }

    pub(super) fn record(&mut self, sample_count: u64, _force: bool) -> io::Result<()> {
        self.sample_count = sample_count;
        self.append_record("active", sample_count)
    }

    pub(super) fn complete(mut self, sample_count: u64) -> io::Result<()> {
        self.append_record("complete", sample_count)
    }

    pub(super) fn discard(mut self) -> io::Result<()> {
        self.append_record("discarded", self.sample_count)
    }
}

pub(super) fn checkpoint_journal_complete(path: &Path) -> bool {
    read_journal(path)
        .is_some_and(|journal| matches!(journal.state.as_str(), "complete" | "recovered"))
}
