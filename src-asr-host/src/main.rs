//! Entry point: a dedicated reader thread. It only parses stdin frames and
//! forwards them — audio over a bounded channel, control commands over their
//! own channel — so ingestion never blocks on VAD or whisper decodes. All
//! session state and inference live in `session`; decode/merge primitives
//! live in `inference`.

mod inference;
mod session;

use std::{
    io::{self, Read},
    sync::mpsc,
    thread,
};

use anyhow::{Context, Result, bail};
use atmospeak_asr_protocol::{AsrCommand, MAX_FRAME_SIZE};
use session::AudioFrameMsg;

/// ~60 s of 20 ms frames. Generous enough that a long preview or finalize
/// decode never pushes back on the microphone; bounded so a genuinely wedged
/// worker applies backpressure (and drops get counted upstream) instead of
/// growing memory without limit.
const AUDIO_QUEUE_CAPACITY: usize = 3_000;

fn main() -> Result<()> {
    let stdin = io::stdin();
    let mut input = stdin.lock();

    let (control_tx, control_rx) = mpsc::channel::<AsrCommand>();
    let (audio_tx, audio_rx) = mpsc::sync_channel::<AudioFrameMsg>(AUDIO_QUEUE_CAPACITY);

    let worker_output = io::stdout();
    let worker = thread::Builder::new()
        .name("atmospeak-asr-inference".to_string())
        .spawn(move || {
            let stdout = worker_output;
            let mut output = stdout.lock();
            session::run_worker(control_rx, audio_rx, &mut output)
        })
        .context("failed to start inference worker")?;

    while let Some(command) = read_frame::<AsrCommand>(&mut input)? {
        match command {
            AsrCommand::AudioFrame {
                session_id,
                sequence,
                timestamp_ms,
                pcm_s16le,
            } => {
                if audio_tx
                    .send(AudioFrameMsg {
                        session_id,
                        sequence,
                        timestamp_ms,
                        pcm_s16le,
                    })
                    .is_err()
                {
                    break;
                }
            }
            other => {
                if control_tx.send(other).is_err() {
                    break;
                }
            }
        }
    }
    // Close both channels so the worker exits once its queues drain, then wait
    // for in-flight finalization events to reach stdout before dying.
    drop(audio_tx);
    drop(control_tx);
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("inference worker panicked"))?
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
