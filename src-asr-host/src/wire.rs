use anyhow::{Result, bail};
use atmospeak_asr_protocol::{AsrEvent, MAX_FRAME_SIZE};

use crate::decoder::SAMPLE_RATE;

pub fn pcm_bytes_to_f32(bytes: &[u8]) -> Result<Vec<f32>> {
    if bytes.len() % 2 != 0 {
        bail!("PCM frame has an odd byte count")
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32768.0)
        .collect())
}

pub fn samples_to_ms(samples: usize) -> u64 {
    ((samples as u128 * 1_000) / SAMPLE_RATE as u128) as u64
}

pub fn write_event(output: &mut impl std::io::Write, event: &AsrEvent) -> Result<()> {
    let payload = rmp_serde::to_vec_named(event)?;
    if payload.len() > MAX_FRAME_SIZE {
        bail!("outgoing IPC frame exceeds limit")
    }
    output.write_all(&(payload.len() as u32).to_le_bytes())?;
    output.write_all(&payload)?;
    output.flush()?;
    Ok(())
}

pub fn write_error(
    output: &mut impl std::io::Write,
    session_id: Option<String>,
    message: &str,
    recoverable: bool,
) -> Result<()> {
    write_event(
        output,
        &AsrEvent::Error {
            session_id,
            recoverable,
            message: message.to_string(),
        },
    )
}
