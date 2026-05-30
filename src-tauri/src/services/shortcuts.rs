use anyhow::{Result, anyhow};
use parking_lot::Mutex;
use std::{collections::HashSet, sync::Arc};
use tauri::{AppHandle, Emitter};

#[cfg(not(target_os = "windows"))]
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

use crate::models::ShortcutStatus;

#[derive(Clone)]
struct ShortcutCandidate {
    label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ParsedShortcut {
    ctrl: bool,
    win: bool,
    alt: bool,
    shift: bool,
    key: ShortcutKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShortcutKey {
    Space,
    D,
}

pub fn register_shortcut(
    app: &AppHandle,
    shortcut_status: Arc<Mutex<ShortcutStatus>>,
    shortcuts_paused: Arc<Mutex<bool>>,
    requested_hotkey: &str,
    paused: bool,
) -> ShortcutStatus {
    #[cfg(target_os = "windows")]
    {
        windows_keyboard_hook::register_shortcut(
            app,
            shortcut_status,
            shortcuts_paused,
            requested_hotkey,
            paused,
        )
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = shortcuts_paused;
        let _ = app.global_shortcut().unregister_all();
        let candidates = shortcut_candidates(requested_hotkey);
        let mut failures = Vec::new();

        for candidate in candidates {
            let shortcut = match parse_shortcut(&candidate.label).map(to_tauri_shortcut) {
                Ok(shortcut) => shortcut,
                Err(error) => {
                    failures.push(format!("{}: {error}", candidate.label));
                    continue;
                }
            };

            match app.global_shortcut().register(shortcut) {
                Ok(()) => {
                    let fallback_note = if candidate.label.eq_ignore_ascii_case(requested_hotkey) {
                        String::new()
                    } else {
                        format!(" Requested {requested_hotkey}, using fallback.")
                    };
                    let status = ShortcutStatus {
                        registered: true,
                        hotkey: candidate.label.clone(),
                        paused,
                        message: format!(
                            "Global shortcut registered: {}.{}",
                            candidate.label, fallback_note
                        ),
                    };
                    *shortcut_status.lock() = status.clone();
                    let _ = app.emit("wind-speak://shortcut-status", status.clone());
                    return status;
                }
                Err(error) => failures.push(format!("{}: {error}", candidate.label)),
            }
        }

        let status = ShortcutStatus {
            registered: false,
            hotkey: String::new(),
            paused,
            message: format!(
                "Global shortcut unavailable. Use the floating control or choose a different shortcut. {}",
                failures.join(" / ")
            ),
        };
        eprintln!("{}", status.message);
        *shortcut_status.lock() = status.clone();
        let _ = app.emit("wind-speak://shortcut-status", status.clone());
        status
    }
}

pub fn set_paused(
    app: &AppHandle,
    shortcut_status: Arc<Mutex<ShortcutStatus>>,
    shortcuts_paused: Arc<Mutex<bool>>,
    paused: bool,
) -> ShortcutStatus {
    *shortcuts_paused.lock() = paused;
    let mut status = shortcut_status.lock();
    status.paused = paused;
    status.message = if paused {
        "Global shortcuts paused. Use the floating control, tray, or resume shortcuts.".to_string()
    } else if status.registered {
        format!("Shortcut active: {}.", status.hotkey)
    } else {
        "Global shortcut is unavailable. Use the floating control or choose another shortcut."
            .to_string()
    };
    let next_status = status.clone();
    drop(status);
    let _ = app.emit("wind-speak://shortcut-status", next_status.clone());
    next_status
}

fn shortcut_candidates(requested_hotkey: &str) -> Vec<ShortcutCandidate> {
    let mut labels = vec![requested_hotkey.trim().to_string()];
    labels.extend([
        "Ctrl+Win+Space".to_string(),
        "Ctrl+Alt+Space".to_string(),
        "Ctrl+Shift+Space".to_string(),
        "Ctrl+Alt+D".to_string(),
    ]);

    let mut seen = HashSet::new();
    labels
        .into_iter()
        .filter_map(|label| {
            let normalized = normalize_label(&label).ok()?;
            if !seen.insert(normalized.clone()) || parse_shortcut(&normalized).is_err() {
                return None;
            }
            Some(ShortcutCandidate { label: normalized })
        })
        .collect()
}

fn normalize_label(label: &str) -> Result<String> {
    let parts = label
        .split('+')
        .map(|part| part.trim().to_ascii_lowercase())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() < 2 {
        return Err(anyhow!(
            "shortcut must include at least one modifier and one key"
        ));
    }

    let mut has_ctrl = false;
    let mut has_win = false;
    let mut has_alt = false;
    let mut has_shift = false;
    let mut key = None;
    for part in parts {
        match part.as_str() {
            "ctrl" | "control" => has_ctrl = true,
            "alt" | "option" => has_alt = true,
            "shift" => has_shift = true,
            "win" | "windows" | "super" | "cmd" | "meta" => has_win = true,
            "space" => key = Some("Space"),
            "d" => key = Some("D"),
            _ => return Err(anyhow!("unsupported shortcut part: {part}")),
        }
    }

    let mut modifiers = Vec::new();
    if has_ctrl {
        modifiers.push("Ctrl");
    }
    if has_win {
        modifiers.push("Win");
    }
    if has_alt {
        modifiers.push("Alt");
    }
    if has_shift {
        modifiers.push("Shift");
    }
    let Some(key) = key else {
        return Err(anyhow!("shortcut must include Space or D"));
    };
    if modifiers.is_empty() {
        return Err(anyhow!("shortcut must include a modifier"));
    }
    modifiers.push(key);
    Ok(modifiers.join("+"))
}

fn parse_shortcut(label: &str) -> Result<ParsedShortcut> {
    let mut parsed = ParsedShortcut {
        ctrl: false,
        win: false,
        alt: false,
        shift: false,
        key: ShortcutKey::Space,
    };
    let mut has_key = false;

    for part in label.split('+') {
        match part {
            "Alt" => parsed.alt = true,
            "Ctrl" => parsed.ctrl = true,
            "Shift" => parsed.shift = true,
            "Win" => parsed.win = true,
            "Space" => {
                parsed.key = ShortcutKey::Space;
                has_key = true;
            }
            "D" => {
                parsed.key = ShortcutKey::D;
                has_key = true;
            }
            _ => return Err(anyhow!("unsupported shortcut part: {part}")),
        }
    }

    if !has_key {
        return Err(anyhow!("shortcut must include a key"));
    }
    if !(parsed.ctrl || parsed.win || parsed.alt || parsed.shift) {
        return Err(anyhow!("shortcut must include a modifier"));
    }
    Ok(parsed)
}

#[cfg(not(target_os = "windows"))]
fn to_tauri_shortcut(parsed: ParsedShortcut) -> Shortcut {
    let mut modifiers = Modifiers::empty();
    if parsed.alt {
        modifiers |= Modifiers::ALT;
    }
    if parsed.ctrl {
        modifiers |= Modifiers::CONTROL;
    }
    if parsed.shift {
        modifiers |= Modifiers::SHIFT;
    }
    if parsed.win {
        modifiers |= Modifiers::SUPER;
    }
    let code = match parsed.key {
        ShortcutKey::Space => Code::Space,
        ShortcutKey::D => Code::KeyD,
    };
    Shortcut::new(Some(modifiers), code)
}

#[cfg(target_os = "windows")]
mod windows_keyboard_hook {
    use super::{ParsedShortcut, ShortcutKey, parse_shortcut, shortcut_candidates};
    use crate::models::ShortcutStatus;
    use parking_lot::Mutex as ParkingMutex;
    use std::{
        sync::{Arc, LazyLock, Mutex as StdMutex, mpsc},
        thread,
        time::Duration,
    };
    use tauri::{AppHandle, Emitter};
    use windows::Win32::{
        Foundation::{LPARAM, LRESULT, WPARAM},
        System::Threading::GetCurrentThreadId,
        UI::{
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_D, VK_LCONTROL, VK_LMENU, VK_LSHIFT,
                VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT, VK_SPACE,
            },
            WindowsAndMessaging::{
                CallNextHookEx, GetMessageW, KBDLLHOOKSTRUCT, MSG, PostThreadMessageW,
                SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
                WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
            },
        },
    };

    static HOOK_CONTEXT: LazyLock<StdMutex<HookContext>> =
        LazyLock::new(|| StdMutex::new(HookContext::default()));
    static HOOK_CONTROL: LazyLock<StdMutex<Option<HookControl>>> =
        LazyLock::new(|| StdMutex::new(None));

    #[derive(Clone)]
    struct HookContext {
        app: Option<AppHandle>,
        shortcuts_paused: Option<Arc<ParkingMutex<bool>>>,
        hotkey: ParsedShortcut,
        active: bool,
        capturing: bool,
    }

    impl Default for HookContext {
        fn default() -> Self {
            Self {
                app: None,
                shortcuts_paused: None,
                hotkey: ParsedShortcut {
                    ctrl: true,
                    win: true,
                    alt: false,
                    shift: false,
                    key: ShortcutKey::Space,
                },
                active: false,
                capturing: false,
            }
        }
    }

    #[derive(Clone, Copy)]
    struct HookControl {
        thread_id: u32,
    }

    pub fn register_shortcut(
        app: &AppHandle,
        shortcut_status: Arc<ParkingMutex<ShortcutStatus>>,
        shortcuts_paused: Arc<ParkingMutex<bool>>,
        requested_hotkey: &str,
        paused: bool,
    ) -> ShortcutStatus {
        stop_existing_hook();
        let candidates = shortcut_candidates(requested_hotkey);
        let Some(candidate) = candidates.first() else {
            let status = ShortcutStatus {
                registered: false,
                hotkey: String::new(),
                paused,
                message: "Global shortcut unavailable. Choose Ctrl+Win+Space, Ctrl+Alt+Space, Ctrl+Shift+Space, or Ctrl+Alt+D.".to_string(),
            };
            *shortcut_status.lock() = status.clone();
            let _ = app.emit("wind-speak://shortcut-status", status.clone());
            return status;
        };

        let parsed = match parse_shortcut(&candidate.label) {
            Ok(parsed) => parsed,
            Err(error) => {
                let status = ShortcutStatus {
                    registered: false,
                    hotkey: String::new(),
                    paused,
                    message: format!("Global shortcut unavailable. {error}"),
                };
                *shortcut_status.lock() = status.clone();
                let _ = app.emit("wind-speak://shortcut-status", status.clone());
                return status;
            }
        };

        match start_hook(app.clone(), shortcuts_paused, parsed) {
            Ok(()) => {
                let status = ShortcutStatus {
                    registered: true,
                    hotkey: candidate.label.clone(),
                    paused,
                    message: format!(
                        "Windows push-to-talk hook armed: {}. Hold to dictate, release to paste.",
                        candidate.label
                    ),
                };
                *shortcut_status.lock() = status.clone();
                let _ = app.emit("wind-speak://shortcut-status", status.clone());
                status
            }
            Err(error) => {
                let status = ShortcutStatus {
                    registered: false,
                    hotkey: String::new(),
                    paused,
                    message: format!(
                        "Windows keyboard hook unavailable. Use the floating control or tray. {error}"
                    ),
                };
                *shortcut_status.lock() = status.clone();
                let _ = app.emit("wind-speak://shortcut-status", status.clone());
                status
            }
        }
    }

    fn start_hook(
        app: AppHandle,
        shortcuts_paused: Arc<ParkingMutex<bool>>,
        hotkey: ParsedShortcut,
    ) -> Result<(), String> {
        {
            let mut context = HOOK_CONTEXT
                .lock()
                .map_err(|_| "keyboard hook context lock failed".to_string())?;
            *context = HookContext {
                app: Some(app),
                shortcuts_paused: Some(shortcuts_paused),
                hotkey,
                active: false,
                capturing: false,
            };
        }

        let (ready_tx, ready_rx) = mpsc::channel();
        thread::Builder::new()
            .name("wind-speak-keyboard-hook".to_string())
            .spawn(move || run_hook_thread(ready_tx))
            .map_err(|error| error.to_string())?;

        let thread_id = ready_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "keyboard hook did not start within 2 seconds".to_string())??;
        *HOOK_CONTROL
            .lock()
            .map_err(|_| "keyboard hook control lock failed".to_string())? =
            Some(HookControl { thread_id });
        Ok(())
    }

    fn stop_existing_hook() {
        let control = HOOK_CONTROL
            .lock()
            .ok()
            .and_then(|mut control| control.take());
        if let Some(control) = control {
            unsafe {
                let _ = PostThreadMessageW(control.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
    }

    fn run_hook_thread(ready_tx: mpsc::Sender<Result<u32, String>>) {
        unsafe {
            let thread_id = GetCurrentThreadId();
            let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0) {
                Ok(hook) => hook,
                Err(error) => {
                    let _ = ready_tx.send(Err(error.to_string()));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(thread_id));

            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).0 > 0 {}

            let _ = UnhookWindowsHookEx(hook);
        }
    }

    unsafe extern "system" fn keyboard_proc(
        n_code: i32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        if n_code >= 0 {
            let event = unsafe { *(l_param.0 as *const KBDLLHOOKSTRUCT) };
            if handle_key_event(w_param.0 as u32, event.vkCode) {
                return LRESULT(1);
            }
        }

        unsafe { CallNextHookEx(None, n_code, w_param, l_param) }
    }

    fn handle_key_event(message: u32, event_vk: u32) -> bool {
        let key_down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
        let key_up = matches!(message, WM_KEYUP | WM_SYSKEYUP);
        if !key_down && !key_up {
            return false;
        }

        let mut context = match HOOK_CONTEXT.lock() {
            Ok(context) => context,
            Err(_) => return false,
        };
        let Some(app) = context.app.clone() else {
            return false;
        };
        if context
            .shortcuts_paused
            .as_ref()
            .map(|paused| *paused.lock())
            .unwrap_or(false)
        {
            if context.active {
                context.active = false;
                context.capturing = false;
                let _ = app.emit("wind-speak://shortcut", "released");
            }
            return false;
        }

        let relevant = context.hotkey.is_relevant(event_vk);
        let modifiers_down = context.hotkey.required_modifiers_down(event_vk, key_down);
        if relevant && modifiers_down {
            context.capturing = true;
        }

        let combo_down = context.hotkey.is_down(event_vk, key_down);
        if combo_down && !context.active {
            context.active = true;
            let _ = app.emit("wind-speak://shortcut", "pressed");
            return relevant;
        }
        if context.active && !combo_down {
            context.active = false;
            let _ = app.emit("wind-speak://shortcut", "released");
            return relevant || context.capturing;
        }

        if context.capturing && key_up && relevant && !modifiers_down {
            context.capturing = false;
            return true;
        }

        relevant && (combo_down || context.active || context.capturing)
    }

    impl ParsedShortcut {
        fn is_relevant(self, event_vk: u32) -> bool {
            event_vk == self.key_vk()
                || matches!(
                    event_vk,
                    16 | 17 | 18 | 91 | 92 | 160 | 161 | 162 | 163 | 164 | 165
                )
        }

        fn is_down(self, event_vk: u32, event_is_down: bool) -> bool {
            let target_down = key_down_considering_event(self.key_vk(), event_vk, event_is_down);
            target_down && self.required_modifiers_down(event_vk, event_is_down)
        }

        fn required_modifiers_down(self, event_vk: u32, event_is_down: bool) -> bool {
            (!self.ctrl
                || modifier_down(
                    VK_CONTROL,
                    VK_LCONTROL,
                    VK_RCONTROL,
                    event_vk,
                    event_is_down,
                ))
                && (!self.alt
                    || modifier_down(VK_MENU, VK_LMENU, VK_RMENU, event_vk, event_is_down))
                && (!self.shift
                    || modifier_down(VK_SHIFT, VK_LSHIFT, VK_RSHIFT, event_vk, event_is_down))
                && (!self.win
                    || either_down_considering_event(VK_LWIN, VK_RWIN, event_vk, event_is_down))
        }

        fn key_vk(self) -> u32 {
            match self.key {
                ShortcutKey::Space => vk_value(VK_SPACE),
                ShortcutKey::D => vk_value(VK_D),
            }
        }
    }

    fn modifier_down(
        generic: VIRTUAL_KEY,
        left: VIRTUAL_KEY,
        right: VIRTUAL_KEY,
        event_vk: u32,
        event_is_down: bool,
    ) -> bool {
        key_down_considering_event(vk_value(generic), event_vk, event_is_down)
            || either_down_considering_event(left, right, event_vk, event_is_down)
    }

    fn either_down_considering_event(
        left: VIRTUAL_KEY,
        right: VIRTUAL_KEY,
        event_vk: u32,
        event_is_down: bool,
    ) -> bool {
        let left_vk = vk_value(left);
        let right_vk = vk_value(right);
        if event_vk == left_vk {
            return event_is_down || key_is_down(right_vk);
        }
        if event_vk == right_vk {
            return event_is_down || key_is_down(left_vk);
        }
        key_is_down(left_vk) || key_is_down(right_vk)
    }

    fn key_down_considering_event(key: u32, event_vk: u32, event_is_down: bool) -> bool {
        if event_vk == key {
            event_is_down
        } else {
            key_is_down(key)
        }
    }

    fn key_is_down(vk: u32) -> bool {
        unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
    }

    fn vk_value(key: VIRTUAL_KEY) -> u32 {
        key.0 as u32
    }
}

#[cfg(test)]
mod tests {
    use super::{ShortcutKey, parse_shortcut, shortcut_candidates};

    #[test]
    fn requested_shortcut_is_first_and_deduplicated() {
        let labels = shortcut_candidates("control + alt + space")
            .into_iter()
            .map(|candidate| candidate.label)
            .collect::<Vec<_>>();
        assert_eq!(labels[0], "Ctrl+Alt+Space");
        assert_eq!(
            labels,
            vec![
                "Ctrl+Alt+Space",
                "Ctrl+Win+Space",
                "Ctrl+Shift+Space",
                "Ctrl+Alt+D"
            ]
        );
    }

    #[test]
    fn parses_default_push_to_talk_hotkey() {
        let parsed = parse_shortcut("Ctrl+Win+Space").expect("parse shortcut");
        assert!(parsed.ctrl);
        assert!(parsed.win);
        assert!(!parsed.alt);
        assert_eq!(parsed.key, ShortcutKey::Space);
    }
}
