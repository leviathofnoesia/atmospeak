use std::{ffi::OsStr, path::Path};

use anyhow::{Context, Result, bail};
use atmospeak_asr_protocol::AsrBackend;

use crate::{decoder::SpeechDecoder, wav::WavInput};

pub fn run() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let (model, wav, language) = match args.as_slice() {
        [model_flag, model, wav_flag, wav]
            if model_flag == "--model" && wav_flag == "--transcribe-wav" =>
        {
            (model, wav, None)
        }
        [model_flag, model, wav_flag, wav, language_flag, language]
            if model_flag == "--model"
                && wav_flag == "--transcribe-wav"
                && language_flag == "--language" =>
        {
            (model, wav, Some(language.as_os_str()))
        }
        _ => bail!(
            "usage: atmospeak-asr-host --model <gguf> --transcribe-wav <path> [--language <code>]"
        ),
    };
    let input = WavInput::open(Path::new(wav))?;
    let mut decoder = SpeechDecoder::load(Path::new(model), AsrBackend::Cpu, 0)?;
    let language = match language {
        Some(tag) => OsStr::to_str(tag).context("speech language must be UTF-8")?,
        None => decoder.model().language_or_default("en"),
    };

    let text = if decoder.supports_streaming() {
        // Keep the live decoder's chunking for recovery with a streaming model.
        decoder.start(language)?;
        input.for_each_frame(|frame| {
            if decoder.push(frame)? {
                decoder.advance()?;
            }
            Ok(())
        })?;
        decoder.finish()?
    } else {
        let hint = decoder.language_hint(language)?;
        let limit = decoder.clip_sample_limit()?;
        let mut clip = Vec::new();
        let mut parts = Vec::new();
        input.for_each_frame(|mut frame| {
            while !frame.is_empty() {
                let count = frame.len().min(limit - clip.len());
                clip.extend_from_slice(&frame[..count]);
                frame = &frame[count..];
                if clip.len() == limit {
                    decoder.decode_clip(&clip, hint.as_deref(), &mut parts)?;
                    clip.clear();
                }
            }
            Ok(())
        })?;
        if !clip.is_empty() {
            decoder.decode_clip(&clip, hint.as_deref(), &mut parts)?;
        }
        parts.join(" ")
    };

    println!("{text}");
    Ok(())
}
