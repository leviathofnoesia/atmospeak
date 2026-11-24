use std::{
    ffi::{CStr, CString},
    fs::{File, OpenOptions},
    io,
    mem::MaybeUninit,
    path::Path,
    ptr::{self, NonNull},
};

use anyhow::{Context, Result, ensure};
use atmospeak_asr_protocol::{AsrBackend, PARAKEET_MODEL_BYTES, PARAKEET_MODEL_SHA256};
use sha2::{Digest, Sha256};
use transcribe_cpp_sys as native;

pub const SAMPLE_RATE: usize = 16_000;
// Match the decoder's default buffered chunk (13 x 80 ms).
const CHUNK_SAMPLES: usize = SAMPLE_RATE * 104 / 100;

/// Owns one resident decoder and holds the verified artifact while it is mapped.
/// Only the speech worker calls into the decoder.
pub struct Parakeet {
    decoder: NonNull<native::transcribe_session>,
    pending: Vec<f32>,
    _weights: File,
}

impl Parakeet {
    pub fn load(path: &Path, backend: AsrBackend, threads: u16) -> Result<Self> {
        ensure!(backend == AsrBackend::Cpu, "this Parakeet build supports CPU decoding");
        let path = path.canonicalize().context("resolve Parakeet weights")?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1); // Deny writes and deletion while the weights are mapped.
        }
        let mut weights = options.open(&path).context("open Parakeet weights")?;
        ensure!(weights.metadata()?.len() == PARAKEET_MODEL_BYTES, "invalid Parakeet model size");
        let mut digest = Sha256::new();
        io::copy(&mut weights, &mut digest)?;
        ensure!(format!("{:x}", digest.finalize()) == PARAKEET_MODEL_SHA256, "Parakeet model checksum mismatch");

        let executable = std::env::current_exe()?;
        let directory = executable.parent().context("speech host has no parent directory")?;
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
            check(native::transcribe_init_backends_ex(&backends), "initialize CPU backend")?;

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
            check(native::transcribe_open(path.as_ptr(), &load, &session, &mut decoder), "load model")?;
            NonNull::new(decoder).context("Parakeet returned no decoder")?
        };

        Ok(Self { decoder, pending: Vec::with_capacity(CHUNK_SAMPLES), _weights: weights })
    }

    pub fn start(&mut self) -> Result<()> {
        self.discard_stream();
        // The pinned Parakeet streaming model takes English audio and uses its
        // trained default buffering geometry.
        unsafe {
            let mut run = MaybeUninit::uninit();
            native::transcribe_run_params_init(run.as_mut_ptr());
            let mut run = run.assume_init();
            run.language = c"en".as_ptr();
            let mut stream = MaybeUninit::uninit();
            native::transcribe_stream_params_init(stream.as_mut_ptr());
            check(
                native::transcribe_stream_begin(self.decoder.as_ptr(), &run, &stream.assume_init()),
                "start stream",
            )
        }
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
            native::transcribe_stream_feed(self.decoder.as_ptr(), self.pending.as_ptr(), count, ptr::null_mut())
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
            check(native::transcribe_stream_finalize(self.decoder.as_ptr(), ptr::null_mut()), "finalize stream")?;
            ensure!(!native::transcribe_was_truncated(self.decoder.as_ptr()), "Parakeet transcript was truncated");
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
            unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
        }
    }
}

impl Drop for Parakeet {
    fn drop(&mut self) {
        unsafe { native::transcribe_close(self.decoder.as_ptr()) };
    }
}

fn path_c_string(path: &Path) -> Result<CString> {
    CString::new(path.to_str().context("Parakeet requires a UTF-8 path")?)
        .context("Parakeet path contains a NUL byte")
}

fn check(status: native::transcribe_status, stage: &str) -> Result<()> {
    if status == native::transcribe_status::TRANSCRIBE_OK {
        return Ok(());
    }
    let description = unsafe { CStr::from_ptr(native::transcribe_status_string(status.0 as i32)) };
    anyhow::bail!("Parakeet {stage}: {}", description.to_string_lossy())
}
