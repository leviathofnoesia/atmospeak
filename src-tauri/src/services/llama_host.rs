//! Bundled local polish runtime: `llama-server` + curated GGUF models.
//!
//! Non-technical path: enable AI edit → download a small model (and the server
//! binary if missing) → resident OpenAI-compatible host on loopback.
//! Technical path still allows Ollama / custom OpenAI-compatible endpoints.

use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};

use crate::services::{app_state::AppState, asr_host, model_downloader, proc};

const HOST_ENV: &str = "ATMOSPEAK_LLAMA_HOST";
const READY_POLL_INTERVAL: Duration = Duration::from_millis(150);
const READY_TIMEOUT: Duration = Duration::from_secs(180);
const INFERENCE_TIMEOUT: Duration = Duration::from_secs(30);

/// Pinned llama.cpp Windows CPU build. Refresh via `scripts/bootstrap-llama.ps1`.
pub const LLAMA_RUNTIME_ZIP_URL: &str =
    "https://github.com/ggml-org/llama.cpp/releases/download/b10178/llama-b10178-bin-win-cpu-x64.zip";

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
    _job: asr_host::job::KillOnClose,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct LlamaHost {
    server_exe: PathBuf,
    model_path: PathBuf,
    running: Mutex<Option<Running>>,
    client: reqwest::blocking::Client,
}

impl LlamaHost {
    pub fn new(server_exe: PathBuf, model_path: PathBuf) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(INFERENCE_TIMEOUT)
            .no_proxy()
            .build()
            .context("failed to build llama host HTTP client")?;
        Ok(Self {
            server_exe,
            model_path,
            running: Mutex::new(None),
            client,
        })
    }

    pub fn endpoint(&self) -> Result<String> {
        let port = self.ensure_running()?;
        Ok(format!("http://127.0.0.1:{port}/v1/chat/completions"))
    }

    pub fn ensure_running(&self) -> Result<u16> {
        let mut guard = self.running.lock();
        if let Some(running) = guard.as_mut() {
            match running.child.try_wait() {
                Ok(None) => return Ok(running.port),
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
            bail!(
                "llama-server not found at {}. Enable AI edit to download it, or run scripts/bootstrap-llama.ps1.",
                self.server_exe.display()
            );
        }
        if !self.model_path.is_file() {
            bail!(
                "polish model not found at {}. Download it from Settings → AI edit.",
                self.model_path.display()
            );
        }

        let port = free_port().context("failed to reserve a loopback port for llama-server")?;
        let mut command = Command::new(&self.server_exe);
        if let Some(runtime_dir) = self.server_exe.parent() {
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
            "-c",
            "2048",
            "-n",
            "256",
            "--parallel",
            "1",
        ]);
        proc::hide_console(&mut command);

        let child = command
            .spawn()
            .with_context(|| format!("failed to start {}", self.server_exe.display()))?;

        #[cfg(target_os = "windows")]
        let job = asr_host::job::attach(&child)?;

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
        let health = format!("http://127.0.0.1:{}/health", running.port);
        let root = format!("http://127.0.0.1:{}/", running.port);
        let deadline = Instant::now() + READY_TIMEOUT;

        while Instant::now() < deadline {
            if let Ok(Some(status)) = running.child.try_wait() {
                return Err(anyhow!("llama-server exited during startup ({status})"));
            }
            if self
                .client
                .get(&health)
                .timeout(Duration::from_secs(2))
                .send()
                .is_ok()
                || self
                    .client
                    .get(&root)
                    .timeout(Duration::from_secs(2))
                    .send()
                    .is_ok()
            {
                return Ok(());
            }
            std::thread::sleep(READY_POLL_INTERVAL);
        }

        Err(anyhow!(
            "llama-server did not become ready within {}s",
            READY_TIMEOUT.as_secs()
        ))
    }

    pub fn shutdown(&self) {
        *self.running.lock() = None;
    }

    pub fn model_path(&self) -> &Path {
        &self.model_path
    }
}

fn free_port() -> Result<u16, std::io::Error> {
    TcpListener::bind("127.0.0.1:0")?
        .local_addr()
        .map(|addr| addr.port())
}

pub fn runtime_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("llama-runtime")
}

pub fn managed_server_path(app_dir: &Path) -> PathBuf {
    runtime_dir(app_dir).join("llama-server.exe")
}

/// Prefer bundled resources; fall back to a managed download under app data.
pub fn resolve_server_exe(app: &AppHandle) -> Option<PathBuf> {
    let candidates = [
        "resources/llama-runtime/llama-server.exe",
        "llama-runtime/llama-server.exe",
    ];
    for relative in candidates {
        if let Ok(path) = app
            .path()
            .resolve(relative, tauri::path::BaseDirectory::Resource)
        {
            if path.is_file() {
                return Some(path);
            }
        }
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
        if dev.is_file() {
            return Some(dev);
        }
    }
    let managed = managed_server_path(&app.state::<AppState>().app_dir);
    managed.is_file().then_some(managed)
}

pub fn ensure_server_binary(app: &AppHandle) -> Result<PathBuf> {
    if let Some(existing) = resolve_server_exe(app) {
        return Ok(existing);
    }

    #[cfg(not(target_os = "windows"))]
    {
        bail!("bundled llama-server auto-install is Windows-only in this release");
    }

    #[cfg(target_os = "windows")]
    {
        download_and_extract_server(app)
    }
}

#[cfg(target_os = "windows")]
fn download_and_extract_server(app: &AppHandle) -> Result<PathBuf> {
    let state = app.state::<AppState>();
    let dest_dir = runtime_dir(&state.app_dir);
    fs::create_dir_all(&dest_dir).context("failed to create llama-runtime directory")?;
    let zip_path = dest_dir.join("llama-server-cpu-x64.zip");

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60 * 30))
        .user_agent("Atmospeak llama-server downloader")
        .build()
        .context("failed to create llama-server download client")?;
    let response = client
        .get(LLAMA_RUNTIME_ZIP_URL)
        .send()
        .and_then(|response| response.error_for_status())
        .context("failed to download llama-server runtime zip")?;
    model_downloader::write_stream_unchecked(response, &zip_path)?;

    expand_archive_windows(&zip_path, &dest_dir)?;
    let _ = fs::remove_file(&zip_path);

    let server = managed_server_path(&state.app_dir);
    if !server.is_file() {
        if let Some(found) = find_file_named(&dest_dir, "llama-server.exe") {
            if found != server {
                fs::copy(&found, &server).context("failed to place llama-server.exe")?;
            }
        }
    }
    if !server.is_file() {
        bail!("llama-server.exe missing after extracting the runtime zip");
    }
    Ok(server)
}

#[cfg(target_os = "windows")]
fn expand_archive_windows(zip_path: &Path, dest_dir: &Path) -> Result<()> {
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                zip_path.display(),
                dest_dir.display()
            ),
        ])
        .status()
        .context("failed to launch PowerShell to extract llama-server")?;
    if !status.success() {
        bail!("Expand-Archive failed while installing llama-server");
    }
    Ok(())
}

fn find_file_named(root: &Path, name: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().and_then(|n| n.to_str()) == Some(name) {
                return Some(path);
            }
        }
    }
    None
}

pub fn publish_host(app: &AppHandle, model_path: PathBuf) -> Result<()> {
    if is_disabled() {
        bail!("bundled llama host is disabled via {HOST_ENV}");
    }
    let server = ensure_server_binary(app)?;
    let host = LlamaHost::new(server, model_path)?;
    app.state::<AppState>()
        .set_llama_host(std::sync::Arc::new(host));
    Ok(())
}
