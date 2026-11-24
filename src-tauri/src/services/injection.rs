use std::{thread, time::Duration};

use anyhow::{anyhow, Context, Result};
use arboard::Clipboard;

use crate::models::InjectionResult;

mod clipboard;
use clipboard::{
    abandon_clipboard_ownership, restore_clipboard_if_owned, write_transcript_to_clipboard,
};

#[derive(Debug, Clone)]
pub struct InjectionTarget {
    pub hwnd: isize,
    pub process_name: Option<String>,
}

/// Strip control characters that can hijack terminals or credential UIs.
/// Keeps newline and tab; drops the rest (including `\r`, ESC, NUL).
pub fn sanitize_for_paste(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_control() || *ch == '\n' || *ch == '\t')
        .collect()
}

pub fn inject_text(
    text: &str,
    restore_clipboard: bool,
    preferred_target: Option<InjectionTarget>,
) -> Result<InjectionResult> {
    inject_text_checked(text, restore_clipboard, preferred_target, || Ok(()))
}

pub fn inject_text_checked(
    text: &str,
    restore_clipboard: bool,
    preferred_target: Option<InjectionTarget>,
    validate: impl Fn() -> Result<()>,
) -> Result<InjectionResult> {
    let text = sanitize_for_paste(text);
    if text.trim().is_empty() {
        return Err(anyhow!("cannot inject an empty transcript"));
    }

    let mut restored_target = false;
    let mut target_process_name = preferred_target
        .as_ref()
        .and_then(|target| target.process_name.clone());

    if let Some(target) = preferred_target.as_ref() {
        match restore_foreground(target) {
            Ok(FocusRestore::Restored) => {
                restored_target = true;
                // Focus just moved; give the target a beat to place caret /
                // activate its edit control before Ctrl+V.
                thread::sleep(Duration::from_millis(40));
            }
            Ok(FocusRestore::AlreadyForeground) => {
                restored_target = true;
            }
            Ok(FocusRestore::Skipped) => {}
            Err(error) => {
                eprintln!("atmospeak injection: restore failed: {error}");
            }
        }
    }

    validate()?;
    let ownership = write_transcript_to_clipboard(&text, restore_clipboard)?;

    if let Err(error) = validate() {
        let _ = restore_clipboard_if_owned(ownership);
        return Err(error);
    }
    match send_paste_shortcut() {
        Ok(()) => {
            // Ctrl+V has already been delivered. Restore the prior clipboard off
            // the critical path so inject_ms / user-visible paste are not
            // inflated by the settle wait the target needs to consume paste.
            if restore_clipboard {
                if ownership.can_restore() {
                    thread::spawn(move || {
                        thread::sleep(Duration::from_millis(350));
                        let _ = restore_clipboard_if_owned(ownership);
                    });
                } else {
                    abandon_clipboard_ownership(ownership);
                }
            } else {
                abandon_clipboard_ownership(ownership);
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
            abandon_clipboard_ownership(ownership);
            if target_process_name.is_none() {
                target_process_name = capture_foreground_target().and_then(|t| t.process_name);
            }
            Ok(InjectionResult {
                injected: false,
                restored_clipboard: false,
                restored_target,
                target_process_name,
                message: format!("Could not paste into the focused app. ({error})"),
            })
        }
    }
}

/// Friendly name of the app owning a window ("Notepad" from `notepad.exe`), used
/// for the dock's "Set down in â€¦" confirmation. Best-effort: a failure here must
/// never affect injection itself.
#[cfg(target_os = "windows")]
fn process_name_for_hwnd(hwnd: isize) -> Option<String> {
    use windows::Win32::{
        Foundation::{CloseHandle, HWND, MAX_PATH},
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
            PROCESS_QUERY_LIMITED_INFORMATION,
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
        use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::GetWindowThreadProcessId};
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(HWND(hwnd as _), Some(&mut pid));
        }
        pid == std::process::id()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd;
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusRestore {
    AlreadyForeground,
    Restored,
    Skipped,
}

/// Restore focus to the dictation target. Distinguishes already-focused (no settle sleep)
/// from a successful activation that needs a short caret settle.
pub fn restore_foreground(target: &InjectionTarget) -> Result<FocusRestore> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            AllowSetForegroundWindow, BringWindowToTop, GetForegroundWindow,
            GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow, ShowWindow,
            ASFW_ANY, SW_RESTORE,
        };

        #[link(name = "user32")]
        unsafe extern "system" {
            fn AttachThreadInput(id_attach: u32, id_attach_to: u32, attach: i32) -> i32;
        }

        if target.hwnd == 0 || !hwnd_is_valid(target.hwnd) {
            return Ok(FocusRestore::Skipped);
        }
        let hwnd = HWND(target.hwnd as *mut _);
        if !unsafe { IsWindow(Some(hwnd)).as_bool() } {
            return Ok(FocusRestore::Skipped);
        }

        // Already foreground â€” paste can proceed without another activation cycle.
        if unsafe { GetForegroundWindow() } == hwnd {
            return Ok(FocusRestore::AlreadyForeground);
        }

        let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
        let current_fg = unsafe { GetForegroundWindow() };
        let fg_thread = unsafe { GetWindowThreadProcessId(current_fg, None) };
        let target_thread = unsafe { GetWindowThreadProcessId(hwnd, None) };
        let attached = fg_thread != 0
            && target_thread != 0
            && fg_thread != target_thread
            && unsafe { AttachThreadInput(fg_thread, target_thread, 1) } != 0;
        // SetForegroundWindow may report success before Windows has completed
        // the foreground transition. Restore and retry briefly, then verify the
        // actual foreground HWND before sending Ctrl+V.
        for _ in 0..8 {
            // SW_RESTORE un-maximizes a maximized window (per Win32 semantics),
            // so only use it to recover a minimized target. Normal and
            // maximized windows keep their geometry untouched.
            if unsafe { IsIconic(hwnd) }.as_bool() {
                let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
            }
            let _ = unsafe { BringWindowToTop(hwnd) };
            let _ = unsafe { SetForegroundWindow(hwnd) };
            if unsafe { GetForegroundWindow() } == hwnd {
                if attached {
                    unsafe {
                        AttachThreadInput(fg_thread, target_thread, 0);
                    }
                }
                return Ok(FocusRestore::Restored);
            }
            thread::sleep(Duration::from_millis(20));
        }
        if attached {
            unsafe {
                AttachThreadInput(fg_thread, target_thread, 0);
            }
        }
        Err(anyhow!(
            "Windows did not restore keyboard focus to the dictation target (elevated/UIPI?)"
        ))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = target;
        Ok(FocusRestore::Skipped)
    }
}

#[cfg(target_os = "windows")]
fn send_paste_shortcut() -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
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
    let sent = unsafe { SendInput(&stages, std::mem::size_of::<INPUT>() as i32) };
    if sent != stages.len() as u32 {
        if sent > 0 {
            let release = [
                keyboard_input(VK_V, KEYEVENTF_KEYUP),
                keyboard_input(VK_CONTROL, KEYEVENTF_KEYUP),
            ];
            let _ = unsafe { SendInput(&release, std::mem::size_of::<INPUT>() as i32) };
        }
        // Never replay a partially inserted shortcut: paste may have happened.
        return Err(anyhow!(
            "Windows SendInput inserted {sent} of {} paste events",
            stages.len()
        ));
    }
    Ok(())
}

pub fn copy_text_to_clipboard(text: &str) -> Result<()> {
    let text = sanitize_for_paste(text);
    if text.trim().is_empty() {
        return Err(anyhow!("cannot copy an empty transcript"));
    }
    Clipboard::new()
        .context("failed to open system clipboard")?
        .set_text(text)
        .context("failed to write transcript to clipboard")
}

#[cfg(not(target_os = "windows"))]
fn send_paste_shortcut() -> Result<()> {
    Err(anyhow!(
        "system-wide paste injection is implemented for Windows in this prototype"
    ))
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::sanitize_for_paste;

    #[test]
    fn sanitize_keeps_newline_and_tab() {
        assert_eq!(sanitize_for_paste("a\nb\tc"), "a\nb\tc");
    }

    #[test]
    fn sanitize_strips_control_chars() {
        assert_eq!(
            sanitize_for_paste("hello\u{0000}\u{001b}\rworld"),
            "helloworld"
        );
    }

    #[test]
    fn clipboard_restores_unchanged_content() {
        let mut pending = super::clipboard::PendingRestore::default();
        let generation = NonZeroU32::new(7);
        let ownership = pending.written(Some("original".into()), generation, true);
        assert!(ownership.can_restore());
        assert_eq!(
            pending.take_restore(ownership, generation).as_deref(),
            Some("original")
        );
    }

    #[test]
    fn clipboard_overlapping_pastes_retain_the_original() {
        let mut pending = super::clipboard::PendingRestore::default();
        let first = pending.written(Some("original".into()), NonZeroU32::new(7), true);
        let previous = pending.snapshot(Some("first transcript".into()), NonZeroU32::new(7));
        let second = pending.written(previous, NonZeroU32::new(8), true);
        assert_eq!(pending.take_restore(first, NonZeroU32::new(8)), None);
        assert_eq!(
            pending.take_restore(second, NonZeroU32::new(8)).as_deref(),
            Some("original")
        );
    }

    #[test]
    fn clipboard_external_writes_break_ownership_even_with_identical_text() {
        for copied in ["same transcript", "a new copy"] {
            let mut pending = super::clipboard::PendingRestore::default();
            let first = pending.written(Some("original".into()), NonZeroU32::new(7), true);
            let previous = pending.snapshot(Some(copied.into()), NonZeroU32::new(8));
            let second = pending.written(previous, NonZeroU32::new(9), true);
            assert_eq!(pending.take_restore(first, NonZeroU32::new(9)), None);
            assert_eq!(
                pending.take_restore(second, NonZeroU32::new(9)).as_deref(),
                Some(copied)
            );
        }
        let mut pending = super::clipboard::PendingRestore::default();
        let ownership = pending.written(Some("original".into()), NonZeroU32::new(7), true);
        assert_eq!(pending.take_restore(ownership, NonZeroU32::new(8)), None);
    }

    #[test]
    fn clipboard_no_restore_write_ends_inheritance_before_delivery_completes() {
        let mut pending = super::clipboard::PendingRestore::default();
        let first = pending.written(Some("original".into()), NonZeroU32::new(7), true);
        let previous = pending.snapshot(Some("first transcript".into()), NonZeroU32::new(7));
        let second = pending.written(previous, NonZeroU32::new(8), false);
        let previous = pending.snapshot(Some("keep this transcript".into()), NonZeroU32::new(8));
        let third = pending.written(previous, NonZeroU32::new(9), true);
        pending.abandon(&second);
        assert_eq!(pending.take_restore(first, NonZeroU32::new(9)), None);
        assert_eq!(
            pending.take_restore(third, NonZeroU32::new(9)).as_deref(),
            Some("keep this transcript")
        );
    }

    #[test]
    fn clipboard_failed_paste_relinquishes_ownership_without_restoration() {
        let mut pending = super::clipboard::PendingRestore::default();
        let ownership = pending.written(Some("original".into()), NonZeroU32::new(7), true);
        pending.abandon(&ownership);
        drop(ownership);
        assert_eq!(
            pending
                .snapshot(Some("transcript".into()), NonZeroU32::new(7))
                .as_deref(),
            Some("transcript")
        );
    }

    #[test]
    fn clipboard_validation_failure_can_rollback_a_no_restore_write() {
        let mut pending = super::clipboard::PendingRestore::default();
        let ownership = pending.written(Some("original".into()), NonZeroU32::new(7), false);
        assert_eq!(
            pending
                .take_restore(ownership, NonZeroU32::new(7))
                .as_deref(),
            Some("original")
        );
    }

    #[test]
    fn clipboard_old_failure_does_not_abandon_a_newer_paste() {
        let mut pending = super::clipboard::PendingRestore::default();
        let first = pending.written(Some("original".into()), NonZeroU32::new(7), true);
        let previous = pending.snapshot(Some("first transcript".into()), NonZeroU32::new(7));
        let second = pending.written(previous, NonZeroU32::new(8), true);
        pending.abandon(&first);
        assert_eq!(
            pending.take_restore(second, NonZeroU32::new(8)).as_deref(),
            Some("original")
        );
    }

    #[test]
    fn clipboard_unknown_sequence_or_unreadable_text_is_not_restorable() {
        let mut pending = super::clipboard::PendingRestore::default();
        let unknown = pending.written(Some("original".into()), None, true);
        assert!(!unknown.can_restore());
        assert_eq!(pending.take_restore(unknown, None), None);
        let known = pending.written(Some("original".into()), NonZeroU32::new(7), true);
        assert_eq!(pending.take_restore(known, None), None);
        let unreadable = pending.written(None, NonZeroU32::new(8), true);
        assert!(!unreadable.can_restore());
        assert_eq!(pending.take_restore(unreadable, NonZeroU32::new(8)), None);
    }

    #[test]
    fn clipboard_failed_write_preserves_only_unchanged_pending_ownership() {
        let mut pending = super::clipboard::PendingRestore::default();
        let first = pending.written(Some("original".into()), NonZeroU32::new(7), true);
        assert!(pending.retain_if_owned(NonZeroU32::new(7)));
        assert_eq!(
            pending.take_restore(first, NonZeroU32::new(7)).as_deref(),
            Some("original")
        );
        let second = pending.written(Some("next original".into()), NonZeroU32::new(8), true);
        assert!(!pending.retain_if_owned(NonZeroU32::new(9)));
        assert_eq!(pending.take_restore(second, NonZeroU32::new(9)), None);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Focused native UI check: opens and closes the existing paste target; no microphone"]
    fn selected_voice_edit_revalidates_and_replaces_only_the_original_range() {
        use std::{
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        use windows::{
            core::PCWSTR,
            Win32::{
                Foundation::{HWND, LPARAM, WPARAM},
                UI::WindowsAndMessaging::{
                    GetForegroundWindow, GetWindow, SendMessageW, SetForegroundWindow, GW_CHILD,
                    WM_SETTEXT,
                },
            },
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let task = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let ready = task.path().join("target.ready");
        let output = task.path().join("target.txt");
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoProfile",
                "-STA",
                "-WindowStyle",
                "Hidden",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(root.join("scripts/native-injection-target.ps1"))
            .arg("-RichText")
            .arg("-OutputPath")
            .arg(&output)
            .arg("-ReadyPath")
            .arg(&ready)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command.spawn().unwrap();
        let job = crate::services::asr_host::job::attach(&child).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.is_file() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let hwnd = HWND(
            std::fs::read_to_string(&ready)
                .unwrap()
                .trim()
                .parse::<isize>()
                .unwrap() as _,
        );
        let previous_window = unsafe { GetForegroundWindow() };
        super::restore_foreground(&super::InjectionTarget {
            hwnd: hwnd.0 as isize,
            process_name: None,
        })
        .unwrap();
        assert_eq!(
            unsafe { GetForegroundWindow() },
            hwnd,
            "verify only our owned target is focused"
        );
        let edit = unsafe { GetWindow(hwnd, GW_CHILD).unwrap() };
        let text: Vec<u16> = "Alpha beta gamma beta"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        unsafe {
            SendMessageW(
                edit,
                WM_SETTEXT,
                Some(WPARAM(0)),
                Some(LPARAM(PCWSTR(text.as_ptr()).0 as _)),
            );
            SendMessageW(edit, 0x00B1, Some(WPARAM(6)), Some(LPARAM(10)));
        }
        std::thread::sleep(Duration::from_millis(250));
        let selected = crate::services::selected_text::SelectedText::capture().unwrap();
        assert_eq!(selected.target.hwnd, hwnd.0 as isize);
        assert_eq!(selected.text, "beta");
        unsafe {
            SendMessageW(edit, 0x00CF, Some(WPARAM(1)), Some(LPARAM(0)));
        }
        assert!(
            crate::services::selected_text::SelectedText::capture().is_err(),
            "read-only text must not be edited"
        );
        unsafe {
            SendMessageW(edit, 0x00CF, Some(WPARAM(0)), Some(LPARAM(0)));
        }

        unsafe {
            SendMessageW(edit, 0x00B1, Some(WPARAM(17)), Some(LPARAM(21)));
        }
        assert!(
            selected.validate().is_err(),
            "same letters at a different range are not the original selection"
        );
        let rejected = super::inject_text_checked("WRONG", true, None, || selected.validate());
        assert!(rejected.is_err());
        assert_eq!(
            std::fs::read_to_string(&output).unwrap(),
            "Alpha beta gamma beta"
        );
        unsafe {
            SendMessageW(edit, 0x00B1, Some(WPARAM(6)), Some(LPARAM(10)));
        }
        let replacement =
            crate::services::voice_edit::direct_edit(&selected.text, "uppercase").unwrap();
        let result =
            super::inject_text_checked(&replacement, true, None, || selected.validate()).unwrap();
        assert!(result.injected);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if std::fs::read_to_string(&output).unwrap_or_default() == "Alpha BETA gamma beta" {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "replacement must appear in the actual native control"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(400));
        if unsafe { GetForegroundWindow() } == hwnd {
            unsafe {
                let _ = SetForegroundWindow(previous_window);
            }
        }
        drop(job);
        let _ = child.wait();
    }
}
