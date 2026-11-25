//! Durable-WAV replay adapter for the native ASR sidecar.
//!
//! The sidecar contract is deliberately small: it receives `--model <path>
//! --transcribe-wav <path> --language <code>`, writes one UTF-8 transcript to stdout, and writes
//! diagnostics to stderr. This module owns the child process and its pipes so
//! cancellation and application shutdown cannot leave native work behind.

use std::{
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;

use crate::services::proc;

pub const DICTATION_CANCELLED: &str = "dictation cancelled";

const POLL_INTERVAL: Duration = Duration::from_millis(25);
const MAX_CAPTURED_OUTPUT_BYTES: usize = 64 * 1024;
const REPLAY_BASE_TIMEOUT: Duration = Duration::from_secs(30);
const REPLAY_MAX_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const REPLAY_BYTES_PER_SECOND: u64 = 32_000;

struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

struct OwnedProcess {
    child: Mutex<Child>,
    status: Mutex<Option<ExitStatus>>,
    stdout: Mutex<Option<JoinHandle<io::Result<CapturedOutput>>>>,
    stderr: Mutex<Option<JoinHandle<io::Result<CapturedOutput>>>>,
    terminated: AtomicBool,
    #[cfg(target_os = "windows")]
    _job: job::KillOnClose,
}

impl OwnedProcess {
    fn spawn(executable: &Path, model_path: &Path, wav_path: &Path, language: &str) -> Result<Self> {
        let mut command = Command::new(executable);
        if let Some(runtime_dir) = executable.parent() {
            command.current_dir(runtime_dir);
        }
        proc::hide_console(&mut command);
        let mut child = command
            .arg("--model")
            .arg(model_path)
            .arg("--transcribe-wav")
            .arg(wav_path)
            .arg("--language")
            .arg(language)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to start ASR sidecar {}", executable.display()))?;

        #[cfg(target_os = "windows")]
        let job = match job::attach(&child) {
            Ok(job) => job,
            Err(error) => {
                kill_child(&mut child);
                return Err(error).context("failed to attach ASR sidecar process job");
            }
        };

        let stdout_pipe = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("ASR sidecar stdout pipe was not created"));
        let stdout_pipe = match stdout_pipe {
            Ok(pipe) => pipe,
            Err(error) => {
                kill_child(&mut child);
                return Err(error);
            }
        };
        let stdout = match spawn_pipe_reader(stdout_pipe, "stdout") {
            Ok(reader) => reader,
            Err(error) => {
                kill_child(&mut child);
                return Err(error).context("failed to start ASR sidecar stdout reader");
            }
        };

        let stderr_pipe = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("ASR sidecar stderr pipe was not created"));
        let stderr_pipe = match stderr_pipe {
            Ok(pipe) => pipe,
            Err(error) => {
                kill_child(&mut child);
                let _ = stdout.join();
                return Err(error);
            }
        };
        let stderr = match spawn_pipe_reader(stderr_pipe, "stderr") {
            Ok(reader) => reader,
            Err(error) => {
                kill_child(&mut child);
                let _ = stdout.join();
                return Err(error).context("failed to start ASR sidecar stderr reader");
            }
        };

        Ok(Self {
            child: Mutex::new(child),
            status: Mutex::new(None),
            stdout: Mutex::new(Some(stdout)),
            stderr: Mutex::new(Some(stderr)),
            terminated: AtomicBool::new(false),
            #[cfg(target_os = "windows")]
            _job: job,
        })
    }

    fn try_wait(&self) -> io::Result<Option<ExitStatus>> {
        if let Some(status) = self.status.lock().as_ref().cloned() {
            return Ok(Some(status));
        }

        let status = self.child.lock().try_wait()?;
        if let Some(status) = status.as_ref() {
            *self.status.lock() = Some(status.clone());
        }
        Ok(status)
    }

    /// Wake as soon as the owned sidecar exits, retaining the bounded interval
    /// for cancellation checks while native inference is still running.
    fn wait_for_exit(&self, timeout: Duration) -> io::Result<()> {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::io::AsRawHandle;
            use windows::Win32::{
                Foundation::{HANDLE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
                System::Threading::WaitForSingleObject,
            };

            let child = self.child.lock();
            let milliseconds = timeout.as_millis().min(u32::MAX as u128) as u32;
            match unsafe { WaitForSingleObject(HANDLE(child.as_raw_handle() as _), milliseconds) } {
                WAIT_OBJECT_0 | WAIT_TIMEOUT => Ok(()),
                WAIT_FAILED => Err(io::Error::last_os_error()),
                result => Err(io::Error::other(format!(
                    "unexpected process wait result: {result:?}"
                ))),
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            thread::sleep(timeout);
            Ok(())
        }
    }

    fn wait(&self) -> io::Result<ExitStatus> {
        if let Some(status) = self.status.lock().as_ref().cloned() {
            return Ok(status);
        }

        let status = self.child.lock().wait()?;
        *self.status.lock() = Some(status.clone());
        Ok(status)
    }

    fn terminate(&self) -> io::Result<ExitStatus> {
        self.terminated.store(true, Ordering::Release);

        if let Some(status) = self.status.lock().as_ref().cloned() {
            return Ok(status);
        }

        let mut child = self.child.lock();
        let status = match child.try_wait() {
            Ok(Some(status)) => Ok(status),
            Ok(None) => {
                let _ = child.kill();
                child.wait()
            }
            Err(error) => {
                let _ = child.kill();
                match child.wait() {
                    Ok(status) => Ok(status),
                    Err(wait_error) => Err(io::Error::new(
                        wait_error.kind(),
                        format!("poll failed: {error}; wait failed: {wait_error}"),
                    )),
                }
            }
        }?;
        *self.status.lock() = Some(status.clone());
        Ok(status)
    }

    fn was_terminated(&self) -> bool {
        self.terminated.load(Ordering::Acquire)
    }

    fn collect_output(&self) -> Result<(CapturedOutput, CapturedOutput)> {
        let stdout = self.stdout.lock().take();
        let stderr = self.stderr.lock().take();
        let stdout_result = join_pipe(stdout);
        let stderr_result = join_pipe(stderr);

        let stdout = stdout_result.context("failed to read ASR sidecar stdout")?;
        let stderr = stderr_result.context("failed to read ASR sidecar stderr")?;
        Ok((stdout, stderr))
    }

    fn wait_and_collect(&self) -> Result<(ExitStatus, CapturedOutput, CapturedOutput)> {
        let status = match self.wait() {
            Ok(status) => status,
            Err(error) => {
                let _ = self.terminate();
                let _ = self.collect_output();
                return Err(error).context("failed to wait for ASR sidecar");
            }
        };
        let (stdout, stderr) = self.collect_output()?;
        Ok((status, stdout, stderr))
    }

    fn terminate_and_drain(&self) -> Result<()> {
        let termination = self.terminate();
        let output = self.collect_output();

        if let Err(error) = termination {
            return Err(error).context("failed to stop ASR sidecar");
        }
        output.map(|_| ())
    }
}

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        let _ = self.terminate_and_drain();
    }
}

pub struct AsrHost {
    executable: PathBuf,
    model_path: PathBuf,
    language: String,
    active: Mutex<Option<Arc<OwnedProcess>>>,
}

impl AsrHost {
    pub fn new(executable: PathBuf, model_path: PathBuf, language: String) -> Result<Self> {
        Ok(Self {
            executable,
            model_path,
            language,
            active: Mutex::new(None),
        })
    }

    /// Validate the lazily used files without warming a model or starting a
    /// process. Existing startup callers use this as a readiness check.
    pub fn ensure_running(&self) -> Result<()> {
        if !self.executable.is_file() {
            return Err(anyhow!(
                "ASR sidecar executable not found at {}",
                self.executable.display()
            ));
        }
        if !super::model_downloader::is_model_file_ready(&self.model_path) {
            return Err(anyhow!(
                "Speech model is missing or has the wrong size at {}",
                self.model_path.display()
            ));
        }
        Ok(())
    }

    pub fn transcribe(&self, wav_path: &Path) -> Result<String> {
        self.transcribe_inner(wav_path, None)
    }

    pub fn transcribe_cancellable(&self, wav_path: &Path, cancel: &AtomicBool) -> Result<String> {
        self.transcribe_inner(wav_path, Some(cancel))
    }

    fn transcribe_inner(&self, wav_path: &Path, cancel: Option<&AtomicBool>) -> Result<String> {
        if is_cancelled(cancel) {
            return Err(anyhow!(DICTATION_CANCELLED));
        }
        self.ensure_running()?;
        if !wav_path.is_file() {
            return Err(anyhow!(
                "durable WAV recording not found at {}",
                wav_path.display()
            ));
        }
        if is_cancelled(cancel) {
            return Err(anyhow!(DICTATION_CANCELLED));
        }

        let replay_timeout = replay_timeout_for(wav_path)?;
        let process = self.start_process(wav_path)?;
        let result = self.finish_process(&process, cancel, replay_timeout);
        self.clear_process(&process);
        result
    }

    fn start_process(&self, wav_path: &Path) -> Result<Arc<OwnedProcess>> {
        let mut active = self.active.lock();
        if active.is_some() {
            return Err(anyhow!("ASR sidecar already has an active transcription"));
        }

        let process = Arc::new(OwnedProcess::spawn(
            &self.executable,
            &self.model_path,
            wav_path,
            &self.language,
        )?);
        *active = Some(process.clone());
        Ok(process)
    }

    fn finish_process(
        &self,
        process: &Arc<OwnedProcess>,
        cancel: Option<&AtomicBool>,
        replay_timeout: Duration,
    ) -> Result<String> {
        let deadline = Instant::now() + replay_timeout;
        loop {
            if is_cancelled(cancel) {
                let _ = process.terminate_and_drain();
                return Err(anyhow!(DICTATION_CANCELLED));
            }

            if Instant::now() >= deadline {
                let timeout_error = || {
                    anyhow!(
                        "ASR sidecar replay timed out after {} seconds",
                        replay_timeout.as_secs()
                    )
                };
                return match process.terminate_and_drain() {
                    Ok(()) => Err(timeout_error()),
                    Err(error) => Err(anyhow!(
                        "{}; failed to clean up timed-out sidecar: {error}",
                        timeout_error()
                    )),
                };
            }

            match process.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if let Err(error) = process.wait_for_exit(POLL_INTERVAL.min(remaining)) {
                        let _ = process.terminate_and_drain();
                        return Err(error).context("failed to wait for ASR sidecar completion");
                    }
                }
                Err(error) => {
                    let _ = process.terminate_and_drain();
                    return Err(error).context("failed to poll ASR sidecar");
                }
            }
        }

        let (status, stdout, stderr) = process.wait_and_collect()?;
        if process.was_terminated() {
            return Err(anyhow!(
                "ASR sidecar was terminated before transcription completed"
            ));
        }
        if is_cancelled(cancel) {
            return Err(anyhow!(DICTATION_CANCELLED));
        }
        if !status.success() {
            return Err(anyhow!(
                "ASR sidecar exited with {status}: {}",
                format_diagnostics(&stderr)
            ));
        }
        if stdout.truncated {
            return Err(anyhow!(
                "ASR sidecar transcript exceeded the {} byte output limit",
                MAX_CAPTURED_OUTPUT_BYTES
            ));
        }

        let transcript = String::from_utf8(stdout.bytes)
            .map_err(|_| anyhow!("ASR sidecar transcript was not valid UTF-8"))?;
        Ok(transcript.trim().to_string())
    }

    fn clear_process(&self, process: &Arc<OwnedProcess>) {
        let mut active = self.active.lock();
        if active
            .as_ref()
            .is_some_and(|candidate| Arc::ptr_eq(candidate, process))
        {
            *active = None;
        }
    }

    pub fn shutdown(&self) {
        let process = self.active.lock().take();
        if let Some(process) = process {
            let _ = process.terminate_and_drain();
        }
    }
}

fn spawn_pipe_reader(
    mut pipe: impl Read + Send + 'static,
    stream: &'static str,
) -> io::Result<JoinHandle<io::Result<CapturedOutput>>> {
    thread::Builder::new()
        .name(format!("atmospeak-asr-{stream}"))
        .spawn(move || {
            let mut bytes = Vec::with_capacity(MAX_CAPTURED_OUTPUT_BYTES.min(8 * 1024));
            let mut buffer = [0_u8; 8 * 1024];
            let mut truncated = false;

            loop {
                let read = pipe.read(&mut buffer)?;
                if read == 0 {
                    break;
                }

                if bytes.len() < MAX_CAPTURED_OUTPUT_BYTES {
                    let remaining = MAX_CAPTURED_OUTPUT_BYTES - bytes.len();
                    let captured = read.min(remaining);
                    bytes.extend_from_slice(&buffer[..captured]);
                    truncated |= captured < read;
                } else {
                    truncated = true;
                }
            }

            Ok(CapturedOutput { bytes, truncated })
        })
}

fn join_pipe(pipe: Option<JoinHandle<io::Result<CapturedOutput>>>) -> Result<CapturedOutput> {
    let Some(reader) = pipe else {
        return Ok(CapturedOutput {
            bytes: Vec::new(),
            truncated: false,
        });
    };
    reader
        .join()
        .map_err(|_| anyhow!("ASR sidecar output reader panicked"))?
        .map_err(|error| anyhow!("ASR sidecar output pipe failed: {error}"))
}

fn format_diagnostics(output: &CapturedOutput) -> String {
    let mut diagnostics = String::from_utf8_lossy(&output.bytes).trim().to_string();
    if diagnostics.is_empty() {
        diagnostics.push_str("no diagnostics");
    }
    if output.truncated {
        diagnostics.push_str(" [stderr truncated]");
    }
    diagnostics
}

fn replay_timeout_for(wav_path: &Path) -> Result<Duration> {
    let source_bytes = std::fs::metadata(wav_path)
        .with_context(|| {
            format!(
                "failed to inspect durable WAV recording at {}",
                wav_path.display()
            )
        })?
        .len();
    Ok(replay_timeout_for_bytes(source_bytes))
}

fn replay_timeout_for_bytes(source_bytes: u64) -> Duration {
    let source_seconds =
        source_bytes.saturating_add(REPLAY_BYTES_PER_SECOND - 1) / REPLAY_BYTES_PER_SECOND;
    REPLAY_BASE_TIMEOUT
        .saturating_add(Duration::from_secs(source_seconds))
        .min(REPLAY_MAX_TIMEOUT)
}

fn is_cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|signal| signal.load(Ordering::Acquire))
}

fn kill_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Ties a native sidecar's lifetime to ours. StreamingAsr and LlamaHost share
/// this guard, so keep it in the service module even though the replay adapter
/// also explicitly kills and waits its child.
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

    #[cfg(target_os = "windows")]
    #[test]
    fn completion_wait_wakes_on_exit_and_keeps_live_waits_bounded() {
        use std::io::Write;

        let mut command = Command::new("cmd.exe");
        proc::hide_console(&mut command);
        let child = command
            .args(["/D", "/Q"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("owned command process");
        let guard = job::attach(&child).expect("owned process job");
        let process = OwnedProcess {
            child: Mutex::new(child),
            status: Mutex::new(None),
            stdout: Mutex::new(None),
            stderr: Mutex::new(None),
            terminated: AtomicBool::new(false),
            _job: guard,
        };
        process
            .wait_for_exit(Duration::from_millis(1))
            .expect("live process timeout");
        assert!(process.try_wait().expect("live status").is_none());
        process
            .child
            .lock()
            .stdin
            .take()
            .expect("owned stdin")
            .write_all(b"exit\r\n")
            .expect("request owned process exit");
        let started = Instant::now();
        process
            .wait_for_exit(Duration::from_secs(5))
            .expect("exit notification");
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "exit must wake before the five-second timeout"
        );
        assert!(
            process
                .try_wait()
                .expect("final status")
                .expect("exited")
                .success()
        );
        eprintln!(
            "replay owned-process exit wait: {:.3} ms",
            elapsed.as_secs_f64() * 1_000.0
        );
    }

    #[test]
    fn missing_binary_fails_instead_of_hanging() {
        let host = AsrHost::new(
            PathBuf::from("does-not-exist-asr-sidecar.exe"),
            PathBuf::from("does-not-exist.bin"),
            "en".to_string(),
        )
        .expect("client builds");
        let error = host.ensure_running().expect_err("must not start");
        assert!(
            error
                .to_string()
                .contains("ASR sidecar executable not found")
        );
    }

    #[test]
    fn replay_timeout_scales_with_source_size_and_is_bounded() {
        assert_eq!(replay_timeout_for_bytes(0), Duration::from_secs(30));
        assert_eq!(replay_timeout_for_bytes(32_000), Duration::from_secs(31));
        assert_eq!(
            replay_timeout_for_bytes(32_000 * 1_800),
            Duration::from_secs(30 * 60)
        );
    }
}
