use std::{
    ffi::{CStr, CString},
    fs::{File, OpenOptions},
    io,
    mem::MaybeUninit,
    path::Path,
    ptr::{self, NonNull},
};

use anyhow::{Context, Result, ensure};
use atmospeak_asr_protocol::{
    AsrBackend,
    speech_models::{self, AUTO_LANGUAGE, SpeechModel},
};
use sha2::{Digest, Sha256};
use transcribe_cpp_sys as native;

pub const SAMPLE_RATE: usize = 16_000;
// Match the decoder's default buffered chunk (13 x 80 ms).
const CHUNK_SAMPLES: usize = SAMPLE_RATE * 104 / 100;

/// Owns one resident decoder and holds the verified artifact while it is mapped.
/// Only the speech worker calls into the decoder.
pub struct SpeechDecoder {
    model: &'static SpeechModel,
    decoder: NonNull<native::transcribe_session>,
    native_languages: Vec<String>,
    streaming: bool,
    pending: Vec<f32>,
    _weights: File,
}

impl SpeechDecoder {
    pub fn load(path: &Path, backend: AsrBackend, threads: u16) -> Result<Self> {
        ensure!(
            backend == AsrBackend::Cpu,
            "this speech host supports CPU decoding"
        );
        let path = path.canonicalize().context("resolve speech model")?;
        let model = speech_models::by_path(&path).context("unknown speech model filename")?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1); // Deny writes and deletion while the weights are mapped.
        }
        let mut weights = options.open(&path).context("open speech model weights")?;
        ensure!(
            weights.metadata()?.len() == model.bytes,
            "invalid speech model size"
        );
        let mut digest = Sha256::new();
        io::copy(&mut weights, &mut digest)?;
        ensure!(
            format!("{:x}", digest.finalize()) == model.sha256,
            "speech model checksum mismatch"
        );

        let executable = std::env::current_exe()?;
        let directory = executable
            .parent()
            .context("speech host has no parent directory")?;
        let directory = path_c_string(directory)?;
        let path = path_c_string(&path)?;

        // These initializers fully populate their ABI structs. All strings
        // remain alive for the duration of the corresponding native call.
        let decoder = unsafe {
            let mut backends = MaybeUninit::uninit();
            native::transcribe_backend_init_params_init(backends.as_mut_ptr());
            let mut backends = backends.assume_init();
            backends.artifact_dir = directory.as_ptr();
            backends.allowed_backends = native::TRANSCRIBE_BACKEND_MASK_CPU;
            check(
                native::transcribe_init_backends_ex(&backends),
                "initialize CPU backend",
            )?;

            let mut load = MaybeUninit::uninit();
            native::transcribe_model_load_params_init(load.as_mut_ptr());
            let mut load = load.assume_init();
            load.backend = native::transcribe_backend_request::TRANSCRIBE_BACKEND_CPU;

            let mut session = MaybeUninit::uninit();
            native::transcribe_session_params_init(session.as_mut_ptr());
            let mut session = session.assume_init();
            if threads > 0 {
                session.n_threads = i32::from(threads);
            }

            let mut decoder = ptr::null_mut();
            check(
                native::transcribe_open(path.as_ptr(), &load, &session, &mut decoder),
                "load model",
            )?;
            NonNull::new(decoder).context("speech engine returned no decoder")?
        };

        let mut engine = Self {
            model,
            decoder,
            native_languages: Vec::new(),
            streaming: false,
            pending: Vec::with_capacity(CHUNK_SAMPLES),
            _weights: weights,
        };
        engine.read_capabilities()?;
        Ok(engine)
    }

    fn read_capabilities(&mut self) -> Result<()> {
        unsafe {
            let mut caps = MaybeUninit::uninit();
            native::transcribe_capabilities_init(caps.as_mut_ptr());
            let model = native::transcribe_get_model(self.decoder.as_ptr());
            check(
                native::transcribe_model_get_capabilities(model, caps.as_mut_ptr()),
                "read model capabilities",
            )?;
            let caps = caps.assume_init();
            self.streaming = self.model.details.supports_streaming && caps.supports_streaming;
            if !caps.languages.is_null() && caps.n_languages > 0 {
                for &tag in std::slice::from_raw_parts(caps.languages, caps.n_languages as usize) {
                    if !tag.is_null() {
                        self.native_languages
                            .push(CStr::from_ptr(tag).to_string_lossy().into_owned());
                    }
                }
            }
        }
        Ok(())
    }

    pub fn model(&self) -> &'static SpeechModel {
        self.model
    }

    pub fn supports_streaming(&self) -> bool {
        self.streaming
    }

    pub fn language_hint(&self, language: &str) -> Result<Option<CString>> {
        ensure!(
            self.model.supports_language(language),
            "{} does not support {language}",
            self.model.label
        );
        if language == AUTO_LANGUAGE {
            return Ok(None);
        }
        let normalized = |value: &str| value.to_ascii_lowercase().replace('_', "-");
        let wanted = normalized(language);
        let native_tag = self
            .native_languages
            .iter()
            .find(|tag| normalized(tag) == wanted)
            .or_else(|| {
                self.native_languages
                    .iter()
                    .find(|tag| normalized(tag).split('-').next() == Some(wanted.as_str()))
            });
        Ok(native_tag
            .map(|tag| CString::new(tag.as_str()))
            .transpose()?)
    }

    pub fn start(&mut self, language: &str) -> Result<()> {
        ensure!(
            self.streaming,
            "{} processes audio after recording",
            self.model.label
        );
        let hint = self.language_hint(language)?;
        self.discard_stream();
        unsafe {
            let run = run_params(hint.as_deref());
            let mut stream = MaybeUninit::uninit();
            native::transcribe_stream_params_init(stream.as_mut_ptr());
            check(
                native::transcribe_stream_begin(self.decoder.as_ptr(), &run, &stream.assume_init()),
                "start stream",
            )
        }
    }

    pub fn clip_sample_limit(&self) -> Result<usize> {
        const MAX_CLIP_SAMPLES: usize = 120 * SAMPLE_RATE;
        let mut limits = MaybeUninit::uninit();
        let milliseconds = unsafe {
            native::transcribe_session_limits_init(limits.as_mut_ptr());
            check(
                native::transcribe_session_get_limits(self.decoder.as_ptr(), limits.as_mut_ptr()),
                "read clip limits",
            )?;
            limits.assume_init().effective_max_audio_ms
        };
        let model_limit = usize::try_from(milliseconds)
            .unwrap_or(0)
            .saturating_mul(SAMPLE_RATE / 1_000);
        Ok(if model_limit == 0 {
            MAX_CLIP_SAMPLES
        } else {
            model_limit.min(MAX_CLIP_SAMPLES)
        })
    }

    pub fn decode_clip(
        &self,
        samples: &[f32],
        hint: Option<&CStr>,
        output: &mut Vec<String>,
    ) -> Result<()> {
        let count = i32::try_from(samples.len())?;
        let status = unsafe {
            native::transcribe_run(
                self.decoder.as_ptr(),
                samples.as_ptr(),
                count,
                &run_params(hint),
            )
        };
        let retryable = matches!(
            status,
            native::transcribe_status::TRANSCRIBE_ERR_INPUT_TOO_LONG
                | native::transcribe_status::TRANSCRIBE_ERR_OUTPUT_TRUNCATED
        );
        if retryable && samples.len() > SAMPLE_RATE / 2 {
            let (first, second) = samples.split_at(samples.len() / 2);
            self.decode_clip(first, hint, output)?;
            return self.decode_clip(second, hint, output);
        }
        check(status, "transcribe clip")?;
        let text = self.text();
        let text = text.trim();
        if !text.is_empty() {
            output.push(text.to_string());
        }
        Ok(())
    }

    pub fn push(&mut self, samples: &[f32]) -> Result<bool> {
        self.pending.extend_from_slice(samples);
        Ok(self.pending.len() >= CHUNK_SAMPLES)
    }

    pub fn advance(&mut self) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let count = i32::try_from(self.pending.len())?;
        // The native feed borrows PCM only for this call.
        let result = unsafe {
            native::transcribe_stream_feed(
                self.decoder.as_ptr(),
                self.pending.as_ptr(),
                count,
                ptr::null_mut(),
            )
        };
        self.pending.clear();
        check(result, "feed stream")
    }

    pub fn update(&mut self) -> Result<String> {
        self.advance()?;
        Ok(self.text())
    }

    pub fn finish(&mut self) -> Result<String> {
        self.advance()?;
        unsafe {
            check(
                native::transcribe_stream_finalize(self.decoder.as_ptr(), ptr::null_mut()),
                "finalize stream",
            )?;
            ensure!(
                !native::transcribe_was_truncated(self.decoder.as_ptr()),
                "speech transcript was truncated"
            );
        }
        Ok(self.text())
    }

    pub fn discard_stream(&mut self) {
        self.pending.clear();
        unsafe { native::transcribe_stream_reset(self.decoder.as_ptr()) };
    }

    fn text(&self) -> String {
        // The string is owned by the decoder until it is used again. Copy it
        // before returning so the next feed cannot invalidate the preview.
        let text = unsafe { native::transcribe_full_text(self.decoder.as_ptr()) };
        if text.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned()
        }
    }
}

fn run_params(hint: Option<&CStr>) -> native::transcribe_run_params {
    let mut run = MaybeUninit::uninit();
    let mut run = unsafe {
        native::transcribe_run_params_init(run.as_mut_ptr());
        run.assume_init()
    };
    run.language = hint.map_or(ptr::null(), CStr::as_ptr);
    run
}

impl Drop for SpeechDecoder {
    fn drop(&mut self) {
        unsafe { native::transcribe_close(self.decoder.as_ptr()) };
    }
}

fn path_c_string(path: &Path) -> Result<CString> {
    CString::new(
        path.to_str()
            .context("speech model requires a UTF-8 path")?,
    )
    .context("speech model path contains a NUL byte")
}

fn check(status: native::transcribe_status, stage: &str) -> Result<()> {
    if status == native::transcribe_status::TRANSCRIBE_OK {
        return Ok(());
    }
    let description = unsafe { CStr::from_ptr(native::transcribe_status_string(status.0 as i32)) };
    anyhow::bail!("Speech decoder {stage}: {}", description.to_string_lossy())
}
