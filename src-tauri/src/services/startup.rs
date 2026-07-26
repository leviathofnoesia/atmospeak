use std::process::Command;

use anyhow::{Context, Result, anyhow};

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const APP_VALUE: &str = "Atmospeak";
const LEGACY_APP_VALUE: &str = "Wind Speak";

pub fn set_start_at_login(enabled: bool) -> Result<()> {
    #[cfg(windows)]
    {
        // Best-effort remove legacy Run key name.
        let _ = Command::new("reg.exe")
            .args(["delete", RUN_KEY, "/v", LEGACY_APP_VALUE, "/f"])
            .status();

        let status = if enabled {
            let exe = std::env::current_exe().context("failed to resolve current executable")?;
            let command = format!("\"{}\"", exe.display());
            Command::new("reg.exe")
                .args([
                    "add", RUN_KEY, "/v", APP_VALUE, "/t", "REG_SZ", "/d", &command, "/f",
                ])
                .status()
                .context("failed to configure Windows startup entry")?
        } else {
            Command::new("reg.exe")
                .args(["delete", RUN_KEY, "/v", APP_VALUE, "/f"])
                .status()
                .context("failed to remove Windows startup entry")?
        };

        if !status.success() && enabled {
            return Err(anyhow!("Windows rejected the startup entry update"));
        }
    }

    let _ = enabled;
    Ok(())
}
