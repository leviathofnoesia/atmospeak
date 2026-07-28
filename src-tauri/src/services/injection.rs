use std::{thread, time::Duration};

use anyhow::{Context, Result, anyhow};
use arboard::Clipboard;

use crate::models::InjectionResult;

#[derive(Debug, Clone)]
pub struct InjectionTarget {
    pub hwnd: isize,
    pub process_name: Option<String>,
}

pub fn inject_text(
    text: &str,
    restore_clipboard: bool,
    preferred_target: Option<InjectionTarget>,
) -> Result<InjectionResult> {
    if text.trim().is_empty() {
        return Err(anyhow!("cannot inject an empty transcript"));
    }

    let mut restored_target = false;
    let mut target_process_name = preferred_target
        .as_ref()
        .and_then(|target| target.process_name.clone());

    if let Some(target) = preferred_target.as_ref() {
        match restore_foreground(target) {
            Ok(true) => {
                restored_target = true;
                // `restore_foreground` verifies both the top-level window and
                // its GUI thread's focused control. Leave a small final margin
                // for the target's activation handler before Ctrl+V.
                thread::sleep(Duration::from_millis(75));
            }
            Ok(false) => {}
            Err(error) => {
                eprintln!("atmospeak injection: restore failed: {error}");
            }
        }
    }

    let mut clipboard = Clipboard::new().context("failed to open system clipboard")?;
    let previous_clipboard = clipboard.get_text().ok();
    clipboard
        .set_text(text.to_string())
        .context("failed to write transcript to clipboard")?;

    match send_paste_shortcut() {
        Ok(()) => {
            if restore_clipboard {
                if let Some(previous) = previous_clipboard {
                    thread::sleep(Duration::from_millis(350));
                    let _ = clipboard.set_text(previous);
                }
            }
            Ok(InjectionResult {
                injected: true,
                restored_clipboard: restore_clipboard,
                restored_target,
                target_process_name,
                message: if restored_target {
                    "Transcript pasted into the restored target application.".to_string()
                } else {
                    "Transcript pasted into the focused application.".to_string()
                },
            })
        }
        Err(error) => {
            // Prefer leaving transcript on clipboard so the user is not empty-handed.
            let _ = clipboard.set_text(text.to_string());
            if target_process_name.is_none() {
                target_process_name = capture_foreground_target().and_then(|t| t.process_name);
            }
            Ok(InjectionResult {
                injected: false,
                restored_clipboard: false,
                restored_target,
                target_process_name,
                message: format!(
                    "Could not paste into the focused app — transcript is on the clipboard. ({error})"
                ),
            })
        }
    }
}

/// Friendly name of the app owning a window ("Notepad" from `notepad.exe`), used
/// for the dock's "Set down in …" confirmation. Best-effort: a failure here must
/// never affect injection itself.
#[cfg(target_os = "windows")]
fn process_name_for_hwnd(hwnd: isize) -> Option<String> {
    use windows::Win32::{
        Foundation::{CloseHandle, HWND, MAX_PATH},
        System::Threading::{
            OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
        UI::WindowsAndMessaging::GetWindowThreadProcessId,
    };

    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(HWND(hwnd as _), Some(&mut pid));
        if pid == 0 {
            return None;
        }

        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; MAX_PATH as usize];
        let mut len = buffer.len() as u32;
        let query = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(handle);
        query.ok()?;

        let path = String::from_utf16_lossy(&buffer[..len as usize]);
        let stem = std::path::Path::new(&path).file_stem()?.to_string_lossy();
        if stem.is_empty() {
            return None;
        }
        // "notepad" -> "Notepad"; leave names that already carry capitals alone.
        Some(if stem.chars().any(char::is_uppercase) {
            stem.to_string()
        } else {
            let mut chars = stem.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => return None,
            }
        })
    }
}

/// Resolve an app name for callers that only kept an HWND.
pub fn process_name_for(hwnd: isize) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        process_name_for_hwnd(hwnd)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd;
        None
    }
}

pub fn capture_foreground_target() -> Option<InjectionTarget> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0.is_null() {
            return None;
        }
        let hwnd_value = hwnd.0 as isize;
        if is_atmospeak_hwnd(hwnd_value) {
            return None;
        }
        Some(InjectionTarget {
            hwnd: hwnd_value,
            process_name: process_name_for_hwnd(hwnd_value),
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

pub fn hwnd_is_valid(hwnd: isize) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::IsWindow;
        if hwnd == 0 {
            return false;
        }
        unsafe { IsWindow(Some(HWND(hwnd as *mut _))).as_bool() }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd;
        false
    }
}

pub fn is_atmospeak_hwnd(hwnd: isize) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowTextW};
        if hwnd == 0 {
            return false;
        }
        let hwnd = HWND(hwnd as *mut _);
        let mut title = [0u16; 256];
        let mut class_name = [0u16; 256];
        let title_len = unsafe { GetWindowTextW(hwnd, &mut title) } as usize;
        let class_len = unsafe { GetClassNameW(hwnd, &mut class_name) } as usize;
        let title = String::from_utf16_lossy(&title[..title_len]);
        let class_name = String::from_utf16_lossy(&class_name[..class_len]);
        title.contains("Atmospeak")
            || title.contains("Wind Speak")
            || class_name.to_ascii_lowercase().contains("tauri")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd;
        false
    }
}

/// Returns Ok(true) if restore succeeded, Ok(false) if skipped (invalid), Err on API failure.
pub fn restore_foreground(target: &InjectionTarget) -> Result<bool> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            ASFW_ANY, AllowSetForegroundWindow, BringWindowToTop, GUITHREADINFO,
            GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, IsWindow, SW_RESTORE,
            SetForegroundWindow, ShowWindow,
        };

        if target.hwnd == 0 || !hwnd_is_valid(target.hwnd) {
            return Ok(false);
        }
        let hwnd = HWND(target.hwnd as *mut _);
        if !unsafe { IsWindow(Some(hwnd)).as_bool() } {
            return Ok(false);
        }

        let target_has_keyboard_focus = || {
            let thread_id = unsafe { GetWindowThreadProcessId(hwnd, None) };
            if thread_id == 0 {
                return false;
            }
            let mut info = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            unsafe { GetGUIThreadInfo(thread_id, &mut info) }.is_ok()
                && info.hwndActive == hwnd
                && !info.hwndFocus.0.is_null()
        };

        if unsafe { GetForegroundWindow() } == hwnd && target_has_keyboard_focus() {
            return Ok(true);
        }

        let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
        // SetForegroundWindow may report success before Windows has completed
        // the foreground transition. Restore and retry briefly, then verify the
        // actual foreground HWND before sending Ctrl+V.
        for _ in 0..10 {
            let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
            let _ = unsafe { BringWindowToTop(hwnd) };
            let _ = unsafe { SetForegroundWindow(hwnd) };
            if unsafe { GetForegroundWindow() } == hwnd && target_has_keyboard_focus() {
                return Ok(true);
            }
            thread::sleep(Duration::from_millis(30));
        }
        Err(anyhow!(
            "Windows did not restore keyboard focus to the dictation target (elevated/UIPI?)"
        ))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = target;
        Ok(false)
    }
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

    let stages = [
        keyboard_input(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(VK_V, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(VK_V, KEYEVENTF_KEYUP),
        keyboard_input(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    for (index, input) in stages.into_iter().enumerate() {
        let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if sent != 1 {
            return Err(anyhow!(
                "Windows SendInput failed at paste stage {}",
                index + 1
            ));
        }
        if index == 0 || index == 2 {
            thread::sleep(Duration::from_millis(15));
        }
    }
    Ok(())
}

pub fn copy_text_to_clipboard(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Err(anyhow!("cannot copy an empty transcript"));
    }
    Clipboard::new()
        .context("failed to open system clipboard")?
        .set_text(text.to_string())
        .context("failed to write transcript to clipboard")
}

#[cfg(not(target_os = "windows"))]
fn send_paste_shortcut() -> Result<()> {
    Err(anyhow!(
        "system-wide paste injection is implemented for Windows in this prototype"
    ))
}
