use std::{thread, time::Duration};

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
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
        VIRTUAL_KEY, VK_CONTROL, VK_V,
    };

    fn keyboard_input(key: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    let inputs = [
        keyboard_input(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(VK_V, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(VK_V, KEYEVENTF_KEYUP),
        keyboard_input(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent == inputs.len() as u32 {
        Ok(())
    } else {
        Err(anyhow!(
            "Windows SendInput sent {sent} of {} events",
            inputs.len()
        ))
    }
}

#[cfg(not(target_os = "windows"))]
fn send_paste_shortcut() -> Result<()> {
    Err(anyhow!(
        "system-wide paste injection is implemented for Windows in this prototype"
    ))
}
