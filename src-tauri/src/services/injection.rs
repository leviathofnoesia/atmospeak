use std::{process::Command, thread, time::Duration};

use anyhow::{Context, Result, anyhow};
use arboard::Clipboard;

use crate::models::InjectionResult;

pub fn inject_text(text: &str, restore_clipboard: bool) -> Result<InjectionResult> {
    if text.trim().is_empty() {
        return Err(anyhow!("cannot inject an empty transcript"));
    }

    let mut clipboard = Clipboard::new().context("failed to open system clipboard")?;
    let previous_clipboard = clipboard.get_text().ok();
    clipboard
        .set_text(text.to_string())
        .context("failed to write transcript to clipboard")?;

    send_paste_shortcut().context("failed to send paste shortcut")?;

    if restore_clipboard {
        if let Some(previous) = previous_clipboard {
            thread::sleep(Duration::from_millis(350));
            let _ = clipboard.set_text(previous);
        }
    }

    Ok(InjectionResult {
        injected: true,
        restored_clipboard: restore_clipboard,
        message: "Transcript pasted into the focused application.".to_string(),
    })
}

#[cfg(target_os = "windows")]
fn send_paste_shortcut() -> Result<()> {
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "$shell = New-Object -ComObject WScript.Shell; $shell.SendKeys('^v')",
        ])
        .status()
        .context("failed to launch Windows paste helper")?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("Windows paste helper exited with {status}"))
    }
}

#[cfg(not(target_os = "windows"))]
fn send_paste_shortcut() -> Result<()> {
    Err(anyhow!(
        "system-wide paste injection is implemented for Windows in this prototype"
    ))
}
