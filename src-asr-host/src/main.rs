//! Entry point: a dedicated reader thread. It only parses stdin frames and
//! forwards them — audio over a bounded channel, control commands over their
//! own channel — so ingestion never blocks on native Moonshine updates. All
//! session state lives in `moonshine_session`; the C ABI bridge lives in
//! `moonshine`.

mod parakeet;
mod session;
mod wav;
mod wire;

use std::{
    io::{self, Read},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Instant,
};

use anyhow::{Context, Result, bail};
use atmospeak_asr_protocol::{AsrCommand, MAX_FRAME_SIZE};
use session::{AudioFrameMsg, ControlMsg};

/// ~60 s of 20 ms frames. Generous enough that a long preview or finalize
/// decode never pushes back on the microphone; bounded so a genuinely wedged
/// worker applies backpressure (and drops get counted upstream) instead of
/// growing memory without limit.
const AUDIO_QUEUE_CAPACITY: usize = 3_000;

fn main() -> Result<()> {
    if std::env::args_os().len() > 1 {
        return run_batch_command();
    }
    let stdin = io::stdin();
    let mut input = stdin.lock();

    let (control_tx, control_rx) = mpsc::channel::<ControlMsg>();
    let (audio_tx, audio_rx) = mpsc::sync_channel::<AudioFrameMsg>(AUDIO_QUEUE_CAPACITY);
    // Shared with the worker so StopSession/CancelSession can mark the active
    // native call before the client kills this owned process when necessary.
    let abort = Arc::new(AtomicBool::new(false));
    let cancel_epoch = Arc::new(AtomicU64::new(0));
    let worker_abort = Arc::clone(&abort);
    let worker_cancel_epoch = Arc::clone(&cancel_epoch);
    // Only abort when the command targets the active session — a mismatched
    // Stop/Cancel must not poison subsequent commits on a still-live session.
    let active_session = Arc::new(Mutex::new(None::<String>));
    let worker_active = Arc::clone(&active_session);

    let worker_output = io::stdout();
    let worker = thread::Builder::new()
        .name("atmospeak-asr-inference".to_string())
        .spawn(move || {
            let stdout = worker_output;
            let mut output = stdout.lock();
            session::run_worker(
                control_rx,
                audio_rx,
                worker_abort,
                worker_cancel_epoch,
                worker_active,
                &mut output,
            )
        })
        .context("failed to start inference worker")?;
    let worker_thread = worker.thread().clone();

    while let Some(command) = read_frame::<AsrCommand>(&mut input)? {
        if dispatch_command(
            command,
            &audio_tx,
            &control_tx,
            &abort,
            &cancel_epoch,
            &active_session,
            &worker_thread,
        )
        .is_err()
        {
            break;
        }
    }
    // Close both channels so the worker exits once its queues drain, then wait
    // for in-flight finalization events to reach stdout before dying.
    drop(audio_tx);
    drop(control_tx);
    worker_thread.unpark();
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("inference worker panicked"))?
}

fn run_batch_command() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 4 || args[0] != "--model" || args[2] != "--transcribe-wav" {
        bail!("usage: atmospeak-asr-host --model <gguf> --transcribe-wav <path>");
    }
    let model_path = std::path::PathBuf::from(&args[1]);
    let wav_path = std::path::PathBuf::from(&args[3]);
    let input = wav::WavInput::open(&wav_path)?;
    let mut engine =
        parakeet::Parakeet::load(&model_path, atmospeak_asr_protocol::AsrBackend::Cpu, 0)?;
    engine.start()?;
    input
        .for_each_frame(|frame| {
            if engine.push(frame)? {
                engine.advance()?;
            }
            Ok(())
        })
        .with_context(|| format!("failed to replay {}", wav_path.display()))?;
    let text = engine.finish()?;
    println!("{text}");
    Ok(())
}

fn dispatch_command(
    command: AsrCommand,
    audio_tx: &mpsc::SyncSender<AudioFrameMsg>,
    control_tx: &mpsc::Sender<ControlMsg>,
    abort: &Arc<AtomicBool>,
    cancel_epoch: &Arc<AtomicU64>,
    active_session: &Arc<Mutex<Option<String>>>,
    worker_thread: &thread::Thread,
) -> Result<()> {
    let received_at = Instant::now();
    match command {
        AsrCommand::AudioFrame {
            session_id,
            sequence,
            timestamp_ms: _,
            pcm_s16le,
        } => {
            match audio_tx.try_send(AudioFrameMsg {
                session_id,
                sequence,
                received_at,
                pcm_s16le,
            }) {
                Ok(()) | Err(mpsc::TrySendError::Full(_)) => {
                    // A full queue is an intentional loss boundary. The
                    // sequence remains in the StopSession barrier so the
                    // worker can report a truthful tail-drop count.
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return Err(anyhow::anyhow!("audio queue unavailable: disconnected"));
                }
            }
        }
        other => {
            let active_target = match &other {
                AsrCommand::StopSession { session_id, .. }
                | AsrCommand::CancelSession { session_id } => active_session
                    .lock()
                    .ok()
                    .and_then(|guard| guard.as_ref().map(|id| id == session_id))
                    .unwrap_or(false),
                _ => false,
            };
            if active_target {
                abort.store(true, Ordering::Release);
                if matches!(&other, AsrCommand::CancelSession { .. }) {
                    cancel_epoch.fetch_add(1, Ordering::AcqRel);
                }
            }
            control_tx
                .send(ControlMsg {
                    command: other,
                    received_at,
                })
                .map_err(|error| anyhow::anyhow!("control queue unavailable: {error}"))?;
        }
    }
    worker_thread.unpark();
    Ok(())
}

fn read_frame<T: serde::de::DeserializeOwned>(input: &mut impl Read) -> Result<Option<T>> {
    let mut length = [0_u8; 4];
    match input.read_exact(&mut length) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME_SIZE {
        bail!("invalid IPC frame length: {length}");
    }
    let mut payload = vec![0; length];
    input.read_exact(&mut payload)?;
    rmp_serde::from_slice(&payload)
        .context("invalid IPC frame")
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use atmospeak_asr_protocol::PROTOCOL_VERSION;

    #[test]
    fn full_audio_queue_does_not_block_control_dispatch() {
        let (audio_tx, audio_rx) = mpsc::sync_channel(1);
        let (control_tx, control_rx) = mpsc::channel();
        let abort = Arc::new(AtomicBool::new(false));
        let cancel_epoch = Arc::new(AtomicU64::new(0));
        let active = Arc::new(Mutex::new(Some("session".to_string())));
        let worker_thread = thread::current();
        let reader = thread::spawn({
            let abort = Arc::clone(&abort);
            let active = Arc::clone(&active);
            move || {
                dispatch_command(
                    AsrCommand::AudioFrame {
                        session_id: "session".to_string(),
                        sequence: 0,
                        timestamp_ms: 0,
                        pcm_s16le: vec![0, 0],
                    },
                    &audio_tx,
                    &control_tx,
                    &abort,
                    &cancel_epoch,
                    &active,
                    &worker_thread,
                )?;
                dispatch_command(
                    AsrCommand::AudioFrame {
                        session_id: "session".to_string(),
                        sequence: 1,
                        timestamp_ms: 20,
                        pcm_s16le: vec![0, 0],
                    },
                    &audio_tx,
                    &control_tx,
                    &abort,
                    &cancel_epoch,
                    &active,
                    &worker_thread,
                )?;
                dispatch_command(
                    AsrCommand::StopSession {
                        session_id: "session".to_string(),
                        last_sequence: Some(1),
                    },
                    &audio_tx,
                    &control_tx,
                    &abort,
                    &cancel_epoch,
                    &active,
                    &worker_thread,
                )
            }
        });

        let control = control_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .expect("control must be dispatched while audio queue is full");
        assert!(matches!(
            control.command,
            AsrCommand::StopSession {
                last_sequence: Some(1),
                ..
            }
        ));
        assert!(matches!(audio_rx.try_recv(), Ok(_)));
        assert!(audio_rx.try_recv().is_err());
        drop(audio_rx);
        reader.join().expect("reader thread").expect("dispatch");
    }

    #[test]
    fn wav_frames_preserve_downmix_resampling_and_output_geometry() {
        use std::io::Cursor;
        for rate in [8_000, 16_000, 22_050, 44_100, 48_000, 192_000] {
            for channels in [1, 2, 6] {
                for frames in [0, 1, 2, 1_003] {
                    let source = (0..frames * channels as usize)
                        .map(|index| ((index * 1_999 % 65_536) as i32 - 32_768) as i16)
                        .collect::<Vec<_>>();
                    let mut encoded = Cursor::new(Vec::new());
                    {
                        let mut writer = hound::WavWriter::new(
                            &mut encoded,
                            hound::WavSpec {
                                channels,
                                sample_rate: rate,
                                bits_per_sample: 16,
                                sample_format: hound::SampleFormat::Int,
                            },
                        )
                        .unwrap();
                        for sample in &source {
                            writer.write_sample(*sample).unwrap();
                        }
                        writer.finalize().unwrap();
                    }
                    let mono = source
                        .chunks(channels as usize)
                        .map(|frame| {
                            frame
                                .iter()
                                .map(|sample| *sample as f32 / 32768.0)
                                .sum::<f32>()
                                / channels as f32
                        })
                        .collect::<Vec<_>>();
                    let expected = if rate == parakeet::SAMPLE_RATE as u32 || mono.is_empty() {
                        mono
                    } else {
                        let length = mono.len() * parakeet::SAMPLE_RATE / rate as usize;
                        (0..length)
                            .map(|index| {
                                let position =
                                    index as f64 * rate as f64 / parakeet::SAMPLE_RATE as f64;
                                let left = position.floor() as usize;
                                let fraction = (position - left as f64) as f32;
                                let a = mono[left.min(mono.len() - 1)];
                                let b = mono[(left + 1).min(mono.len() - 1)];
                                a + (b - a) * fraction
                            })
                            .collect()
                    };
                    encoded.set_position(0);
                    let mut actual = Vec::new();
                    let mut lengths = Vec::new();
                    wav::WavInput::new(encoded)
                        .unwrap()
                        .for_each_frame(|frame| {
                            lengths.push(frame.len());
                            actual.extend(frame.iter().map(|sample| sample.to_bits()));
                            Ok(())
                        })
                        .unwrap();
                    assert_eq!(
                        actual,
                        expected.iter().map(|s| s.to_bits()).collect::<Vec<_>>(),
                        "rate={rate} channels={channels} frames={frames}"
                    );
                    assert!(
                        lengths
                            .iter()
                            .all(|length| *length > 0 && *length <= parakeet::SAMPLE_RATE / 50)
                    );
                    assert!(
                        lengths
                            .iter()
                            .rev()
                            .skip(1)
                            .all(|length| *length == parakeet::SAMPLE_RATE / 50)
                    );
                }
            }
        }
    }

    #[test]
    fn wav_stream_rejects_truncated_unused_tail_and_propagates_consumer_failure() {
        use std::io::Cursor;
        assert!(wav::WavInput::new(Cursor::new(b"not a WAV")).is_err());
        let mut encoded = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(
                &mut encoded,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 192_000,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for sample in 0..12_i16 {
                writer.write_sample(sample).unwrap();
            }
            writer.finalize().unwrap();
        }
        let bytes = encoded.into_inner();
        // This downsample has only one output, which refers to the first two
        // input samples. Its unused tail must still be checked for truncation.
        let truncated = &bytes[..bytes.len() - 1];
        assert!(
            wav::WavInput::new(Cursor::new(truncated))
                .unwrap()
                .validate_extent(truncated.len() as u64)
                .is_err()
        );
        wav::WavInput::new(Cursor::new(&bytes))
            .unwrap()
            .validate_extent(bytes.len() as u64)
            .unwrap();
        assert!(
            wav::WavInput::new(Cursor::new(truncated))
                .unwrap()
                .for_each_frame(|_| Ok(()))
                .is_err()
        );
        let error = wav::WavInput::new(Cursor::new(bytes.clone()))
            .unwrap()
            .for_each_frame(|_| bail!("consumer stopped"))
            .unwrap_err();
        assert!(error.to_string().contains("consumer stopped"));
        let mut zero_rate = bytes;
        zero_rate[24..28].fill(0);
        assert!(wav::WavInput::new(Cursor::new(zero_rate)).is_err());
        let mut unsupported = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(
                &mut unsupported,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 16_000,
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                },
            )
            .unwrap();
            writer.write_sample(0.5_f32).unwrap();
            writer.finalize().unwrap();
        }
        unsupported.set_position(0);
        assert!(wav::WavInput::new(unsupported).is_err());
    }

    #[test]
    fn stop_barrier_round_trips_over_the_ipc_wire() {
        let command = AsrCommand::StopSession {
            session_id: "session".to_string(),
            last_sequence: Some(17),
        };
        let payload = rmp_serde::to_vec_named(&command).expect("encode command");
        let mut wire = (payload.len() as u32).to_le_bytes().to_vec();
        wire.extend_from_slice(&payload);

        let decoded = read_frame::<AsrCommand>(&mut wire.as_slice())
            .expect("read command")
            .expect("command frame");
        assert_eq!(decoded, command);
        assert_eq!(PROTOCOL_VERSION, 2);
    }
}
