//! Phase B: a resident `whisper-server` process so the model stays loaded.
//!
//! The Phase A backend spawns `whisper-cli.exe` per utterance, which reloads the
//! ~141 MB model every time and dominates latency. This module keeps one server
//! alive on loopback and posts each WAV to it instead.
//!
//! The host is always optional. Every failure path degrades to the CLI backend in
//! `transcriber.rs` rather than breaking dictation.

use std::{
    io::ErrorKind,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;

use crate::services::proc;

/// Set `ATMOSPEAK_WHISPER_HOST=0` to force the one-shot CLI backend.
const HOST_ENV: &str = "ATMOSPEAK_WHISPER_HOST";

/// Timeout budgets scale with model size so advanced runtime overrides and the
/// largest managed models do not fall back merely because a CPU-only machine is
/// slow. Base/tiny remain bounded more tightly.
const SMALL_MODEL_MAX_BYTES: u64 = 200 * 1024 * 1024;
const MEDIUM_MODEL_MAX_BYTES: u64 = 700 * 1024 * 1024;
const READY_POLL_INTERVAL: Duration = Duration::from_millis(150);

pub fn is_disabled() -> bool {
    matches!(
        std::env::var(HOST_ENV).ok().as_deref(),
        Some("0") | Some("false")
    )
}

struct Running {
    child: Child,
    port: u16,
    #[cfg(target_os = "windows")]
    _job: job::KillOnClose,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct AsrHost {
    server_exe: PathBuf,
    model_path: PathBuf,
    ready_timeout: Duration,
    running: Mutex<Option<Running>>,
    client: reqwest::blocking::Client,
}

impl AsrHost {
    pub fn new(server_exe: PathBuf, model_path: PathBuf) -> Result<Self> {
        let (ready_timeout, inference_timeout) = model_timeout_budget(&model_path);
        let client = reqwest::blocking::Client::builder()
            .timeout(inference_timeout)
            // Loopback only; a proxy would break the connection and is never wanted here.
            .no_proxy()
            .build()
            .context("failed to build ASR host HTTP client")?;
        Ok(Self {
            server_exe,
            model_path,
            ready_timeout,
            running: Mutex::new(None),
            client,
        })
    }

    /// Start the server if it is not already up. Safe to call repeatedly; this is
    /// how a host that died between utterances gets respawned.
    pub fn ensure_running(&self) -> Result<u16> {
        let mut guard = self.running.lock();

        if let Some(running) = guard.as_mut() {
            match running.child.try_wait() {
                // Still alive.
                Ok(None) => return Ok(running.port),
                // Exited on its own — fall through and respawn.
                Ok(Some(_)) | Err(_) => {
                    *guard = None;
                }
            }
        }

        let running = self.spawn()?;
        let port = running.port;
        *guard = Some(running);
        Ok(port)
    }

    fn spawn(&self) -> Result<Running> {
        if !self.server_exe.is_file() {
            return Err(anyhow!(
                "whisper-server.exe not found at {}",
                self.server_exe.display()
            ));
        }
        if !self.model_path.is_file() {
            return Err(anyhow!(
                "speech model not found at {}",
                self.model_path.display()
            ));
        }

        let port = free_port().context("failed to reserve a loopback port")?;
        let mut command = Command::new(&self.server_exe);
        if let Some(runtime_dir) = self.server_exe.parent() {
            // Same reason as the CLI path: the ggml DLLs sit next to the binary.
            command.current_dir(runtime_dir);
        }
        command.args([
            "-m",
            self.model_path
                .to_str()
                .ok_or_else(|| anyhow!("model path contains invalid unicode"))?,
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ]);
        proc::hide_console(&mut command);

        let child = command
            .spawn()
            .with_context(|| format!("failed to start {}", self.server_exe.display()))?;

        #[cfg(target_os = "windows")]
        let job = job::attach(&child)?;

        let mut running = Running {
            child,
            port,
            #[cfg(target_os = "windows")]
            _job: job,
        };

        self.wait_until_ready(&mut running)?;
        Ok(running)
    }

    fn wait_until_ready(&self, running: &mut Running) -> Result<()> {
        let url = format!("http://127.0.0.1:{}/", running.port);
        let deadline = Instant::now() + self.ready_timeout;

        while Instant::now() < deadline {
            if let Ok(Some(status)) = running.child.try_wait() {
                return Err(anyhow!("whisper-server exited during startup ({status})"));
            }
            // Any HTTP response means the listener is up, which the server only does
            // after the model is loaded.
            if self
                .client
                .get(&url)
                .timeout(Duration::from_secs(2))
                .send()
                .is_ok()
            {
                return Ok(());
            }
            std::thread::sleep(READY_POLL_INTERVAL);
        }

        Err(anyhow!(
            "whisper-server did not become ready within {}s",
            self.ready_timeout.as_secs()
        ))
    }

    /// Transcribe a WAV via the resident server.
    ///
    /// Returns the transcript, which may legitimately be empty when the recording
    /// held no speech — that is a result, not a host failure. Errors mean the host
    /// itself misbehaved; the running server is torn down so the next call respawns
    /// it, and the caller falls back to the CLI.
    pub fn transcribe(&self, wav_path: &Path) -> Result<String> {
        let port = self.ensure_running()?;

        match self.post_inference(port, wav_path) {
            Ok(text) => Ok(text),
            Err(error) => {
                // Drop the child so `ensure_running` starts a fresh one next time.
                *self.running.lock() = None;
                Err(error)
            }
        }
    }

    fn post_inference(&self, port: u16, wav_path: &Path) -> Result<String> {
        let file_name = wav_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "audio.wav".to_string());
        let bytes = std::fs::read(wav_path)
            .with_context(|| format!("failed to read {}", wav_path.display()))?;

        let part = reqwest::blocking::multipart::Part::bytes(bytes)
            .file_name(file_name)
            .mime_str("audio/wav")?;
        let form = reqwest::blocking::multipart::Form::new()
            .part("file", part)
            .text("response_format", "text");

        let response = self
            .client
            .post(format!("http://127.0.0.1:{port}/inference"))
            .multipart(form)
            .send()
            .context("whisper-server request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(anyhow!("whisper-server returned {status}: {}", body.trim()));
        }

        Ok(response
            .text()
            .context("failed to read whisper-server response")?
            .trim()
            .to_string())
    }

    pub fn shutdown(&self) {
        *self.running.lock() = None;
    }
}

fn model_timeout_budget(model_path: &Path) -> (Duration, Duration) {
    let bytes = std::fs::metadata(model_path)
        .map(|metadata| metadata.len())
        .unwrap_or(u64::MAX);
    timeout_budget_for_model_bytes(bytes)
}

fn timeout_budget_for_model_bytes(bytes: u64) -> (Duration, Duration) {
    if bytes <= SMALL_MODEL_MAX_BYTES {
        (Duration::from_secs(120), Duration::from_secs(300))
    } else if bytes <= MEDIUM_MODEL_MAX_BYTES {
        (Duration::from_secs(240), Duration::from_secs(600))
    } else {
        (Duration::from_secs(600), Duration::from_secs(1_800))
    }
}

/// Reserve an ephemeral loopback port by binding and immediately releasing it.
fn free_port() -> Result<u16, std::io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    if port == 0 {
        return Err(std::io::Error::new(
            ErrorKind::AddrNotAvailable,
            "no ephemeral port available",
        ));
    }
    Ok(port)
}

/// Ties the server's lifetime to ours. Without a job object, a crashed or
/// force-killed Atmospeak would leave whisper-server resident holding the model.
#[cfg(target_os = "windows")]
pub(super) mod job {
    use anyhow::{Context, Result};
    use std::{os::windows::io::AsRawHandle, process::Child};
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        },
    };

    pub struct KillOnClose(HANDLE);

    // The handle is owned solely by this struct and only closed on drop.
    unsafe impl Send for KillOnClose {}
    unsafe impl Sync for KillOnClose {}

    impl Drop for KillOnClose {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub fn attach(child: &Child) -> Result<KillOnClose> {
        unsafe {
            let job = CreateJobObjectW(None, None).context("CreateJobObjectW failed")?;
            let guard = KillOnClose(job);

            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .context("SetInformationJobObject failed")?;

            AssignProcessToJobObject(job, HANDLE(child.as_raw_handle() as _))
                .context("AssignProcessToJobObject failed")?;

            Ok(guard)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_port_returns_a_usable_port() {
        let port = free_port().expect("ephemeral port");
        assert!(port > 0);
        // Released immediately, so it must be bindable again.
        TcpListener::bind(("127.0.0.1", port)).expect("port should be free after release");
    }

    #[test]
    fn missing_binary_fails_instead_of_hanging() {
        let host = AsrHost::new(
            PathBuf::from("does-not-exist-whisper-server.exe"),
            PathBuf::from("does-not-exist.bin"),
        )
        .expect("client builds");
        let error = host.ensure_running().expect_err("must not start");
        assert!(error.to_string().contains("whisper-server.exe not found"));
    }

    #[test]
    fn timeout_budget_scales_for_large_and_advanced_models() {
        assert_eq!(
            timeout_budget_for_model_bytes(150 * 1024 * 1024),
            (Duration::from_secs(120), Duration::from_secs(300))
        );
        assert_eq!(
            timeout_budget_for_model_bytes(500 * 1024 * 1024),
            (Duration::from_secs(240), Duration::from_secs(600))
        );
        assert_eq!(
            timeout_budget_for_model_bytes(2 * 1024 * 1024 * 1024),
            (Duration::from_secs(600), Duration::from_secs(1_800))
        );
    }
}
