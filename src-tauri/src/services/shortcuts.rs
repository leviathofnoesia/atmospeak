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
    key: Option<ShortcutKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShortcutKey {
    Space,
    D,
}

fn sync_runtime_pause_state(shortcuts_paused: &Arc<Mutex<bool>>, paused: bool) {
    *shortcuts_paused.lock() = paused;
}

pub fn register_shortcut(
    app: &AppHandle,
    shortcut_status: Arc<Mutex<ShortcutStatus>>,
    shortcuts_paused: Arc<Mutex<bool>>,
    requested_hotkey: &str,
    paused: bool,
) -> ShortcutStatus {
    // Registration and the hook must agree about whether input is live. Setup
    // starts globally paused, then temporarily arms the chosen chord for its
    // mandatory test. Updating only ShortcutStatus leaves the Windows hook
    // silently discarding every key event.
    sync_runtime_pause_state(&shortcuts_paused, paused);

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
            let shortcut = match parse_shortcut(&candidate.label).and_then(to_tauri_shortcut) {
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
        "Ctrl+Win".to_string(),
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
            "shortcut must include at least one modifier and one key, or two modifiers"
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
    if modifiers.is_empty() {
        return Err(anyhow!("shortcut must include a modifier"));
    }
    if let Some(key) = key {
        modifiers.push(key);
    } else if modifiers.len() < 2 {
        return Err(anyhow!(
            "modifier-only shortcuts must include at least two modifiers"
        ));
    }
    Ok(modifiers.join("+"))
}

fn parse_shortcut(label: &str) -> Result<ParsedShortcut> {
    let mut parsed = ParsedShortcut {
        ctrl: false,
        win: false,
        alt: false,
        shift: false,
        key: None,
    };

    for part in label.split('+') {
        match part {
            "Alt" => parsed.alt = true,
            "Ctrl" => parsed.ctrl = true,
            "Shift" => parsed.shift = true,
            "Win" => parsed.win = true,
            "Space" => {
                parsed.key = Some(ShortcutKey::Space);
            }
            "D" => {
                parsed.key = Some(ShortcutKey::D);
            }
            _ => return Err(anyhow!("unsupported shortcut part: {part}")),
        }
    }

    if !(parsed.ctrl || parsed.win || parsed.alt || parsed.shift) {
        return Err(anyhow!("shortcut must include a modifier"));
    }
    if parsed.key.is_none()
        && [parsed.ctrl, parsed.win, parsed.alt, parsed.shift]
            .into_iter()
            .filter(|active| *active)
            .count()
            < 2
    {
        return Err(anyhow!(
            "modifier-only shortcuts must include at least two modifiers"
        ));
    }
    Ok(parsed)
}

#[cfg(not(target_os = "windows"))]
fn to_tauri_shortcut(parsed: ParsedShortcut) -> Result<Shortcut> {
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
    let Some(key) = parsed.key else {
        return Err(anyhow!(
            "modifier-only shortcuts are available on Windows only"
        ));
    };
    let code = match key {
        ShortcutKey::Space => Code::Space,
        ShortcutKey::D => Code::KeyD,
    };
    Ok(Shortcut::new(Some(modifiers), code))
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
    use tauri::{AppHandle, Emitter, Manager};
    use windows::Win32::{
        Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
        UI::{
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_D, VK_ESCAPE, VK_LCONTROL, VK_LMENU,
                VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
                VK_SPACE,
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
        runtime: ShortcutRuntimeState,
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
                    key: None,
                },
                runtime: ShortcutRuntimeState::default(),
            }
        }
    }

    #[derive(Clone, Copy, Default)]
    struct ShortcutRuntimeState {
        active: bool,
        capturing: bool,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ShortcutSignal {
        Pressed,
        Released,
        Cancel,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct ShortcutEventOutcome {
        consume: bool,
        signal: Option<ShortcutSignal>,
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
                message: "Global shortcut unavailable. Choose Ctrl+Win, Ctrl+Win+Space, Ctrl+Alt+Space, Ctrl+Shift+Space, or Ctrl+Alt+D.".to_string(),
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
                runtime: ShortcutRuntimeState::default(),
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
            let module = match GetModuleHandleW(None) {
                Ok(module) => HINSTANCE(module.0),
                Err(error) => {
                    let _ = ready_tx.send(Err(error.to_string()));
                    return;
                }
            };
            let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(module), 0)
            {
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
            if context.runtime.active {
                context.runtime = ShortcutRuntimeState::default();
                let _ = app.emit("wind-speak://shortcut", "released");
                crate::services::dictation_engine::route_shortcut_payload(&app, "released");
            }
            return false;
        }

        let hotkey = context.hotkey;
        let outcome = context
            .runtime
            .handle_event(hotkey, event_vk, key_down, key_up, |vk| key_is_down(vk));
        if let Some(signal) = outcome.signal {
            let payload = match signal {
                ShortcutSignal::Pressed => "pressed",
                ShortcutSignal::Released => "released",
                ShortcutSignal::Cancel => "cancel",
            };
            let _ = app.emit("wind-speak://shortcut", payload);
            // Setup validates the hook with the real chord, but must never start
            // dictation or create an overlay before calibration is complete.
            let testing_only = app
                .try_state::<crate::services::app_state::AppState>()
                .is_some_and(|state| state.shortcut_test_active());
            if !testing_only {
                crate::services::dictation_engine::route_shortcut_payload(&app, payload);
            }
        }
        outcome.consume
    }

    impl ShortcutRuntimeState {
        fn handle_event(
            &mut self,
            hotkey: ParsedShortcut,
            event_vk: u32,
            key_down: bool,
            key_up: bool,
            key_is_down: impl Fn(u32) -> bool,
        ) -> ShortcutEventOutcome {
            if self.active && key_down && event_vk == vk_value(VK_ESCAPE) {
                self.active = false;
                self.capturing = false;
                return ShortcutEventOutcome {
                    consume: true,
                    signal: Some(ShortcutSignal::Cancel),
                };
            }

            let relevant = hotkey.is_relevant(event_vk);
            let modifiers_down = hotkey.required_modifiers_down(event_vk, key_down, &key_is_down);
            if relevant && modifiers_down {
                self.capturing = true;
            }

            let combo_down = hotkey.is_down(event_vk, key_down, &key_is_down);
            if combo_down && !self.active {
                self.active = true;
                return ShortcutEventOutcome {
                    consume: relevant,
                    signal: Some(ShortcutSignal::Pressed),
                };
            }
            if self.active && !combo_down {
                self.active = false;
                let consume = relevant || self.capturing;
                self.capturing = false;
                return ShortcutEventOutcome {
                    consume,
                    signal: Some(ShortcutSignal::Released),
                };
            }

            if self.capturing && key_up && relevant && !modifiers_down {
                self.capturing = false;
                return ShortcutEventOutcome {
                    consume: false,
                    signal: None,
                };
            }

            ShortcutEventOutcome {
                // Do not consume a keyed chord's modifiers before its key is
                // pressed. Consumed modifier-down events never enter Windows'
                // async key state, so Space/D could not complete the chord.
                consume: relevant && (combo_down || self.active),
                signal: None,
            }
        }
    }

    impl ParsedShortcut {
        fn is_relevant(self, event_vk: u32) -> bool {
            self.key_vk()
                .map(|key_vk| event_vk == key_vk)
                .unwrap_or(false)
                || matches!(
                    event_vk,
                    16 | 17 | 18 | 91 | 92 | 160 | 161 | 162 | 163 | 164 | 165
                )
        }

        fn is_down(
            self,
            event_vk: u32,
            event_is_down: bool,
            key_is_down: &impl Fn(u32) -> bool,
        ) -> bool {
            let target_down = self
                .key_vk()
                .map(|key_vk| {
                    key_down_considering_event(key_vk, event_vk, event_is_down, key_is_down)
                })
                .unwrap_or(true);
            target_down && self.required_modifiers_down(event_vk, event_is_down, key_is_down)
        }

        fn required_modifiers_down(
            self,
            event_vk: u32,
            event_is_down: bool,
            key_is_down: &impl Fn(u32) -> bool,
        ) -> bool {
            (!self.ctrl
                || modifier_down(
                    VK_CONTROL,
                    VK_LCONTROL,
                    VK_RCONTROL,
                    event_vk,
                    event_is_down,
                    key_is_down,
                ))
                && (!self.alt
                    || modifier_down(
                        VK_MENU,
                        VK_LMENU,
                        VK_RMENU,
                        event_vk,
                        event_is_down,
                        key_is_down,
                    ))
                && (!self.shift
                    || modifier_down(
                        VK_SHIFT,
                        VK_LSHIFT,
                        VK_RSHIFT,
                        event_vk,
                        event_is_down,
                        key_is_down,
                    ))
                && (!self.win
                    || either_down_considering_event(
                        VK_LWIN,
                        VK_RWIN,
                        event_vk,
                        event_is_down,
                        key_is_down,
                    ))
        }

        fn key_vk(self) -> Option<u32> {
            self.key.map(|key| match key {
                ShortcutKey::Space => vk_value(VK_SPACE),
                ShortcutKey::D => vk_value(VK_D),
            })
        }
    }

    fn modifier_down(
        generic: VIRTUAL_KEY,
        left: VIRTUAL_KEY,
        right: VIRTUAL_KEY,
        event_vk: u32,
        event_is_down: bool,
        key_is_down: &impl Fn(u32) -> bool,
    ) -> bool {
        key_down_considering_event(vk_value(generic), event_vk, event_is_down, key_is_down)
            || either_down_considering_event(left, right, event_vk, event_is_down, key_is_down)
    }

    fn either_down_considering_event(
        left: VIRTUAL_KEY,
        right: VIRTUAL_KEY,
        event_vk: u32,
        event_is_down: bool,
        key_is_down: &impl Fn(u32) -> bool,
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

    fn key_down_considering_event(
        key: u32,
        event_vk: u32,
        event_is_down: bool,
        key_is_down: &impl Fn(u32) -> bool,
    ) -> bool {
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

    #[cfg(test)]
    mod tests {
        use super::{
            ShortcutEventOutcome, ShortcutRuntimeState, ShortcutSignal, parse_shortcut, vk_value,
        };
        use std::collections::HashSet;
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            VK_D, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LWIN, VK_SPACE,
        };

        #[test]
        fn modifier_only_ctrl_win_emits_press_and_release() {
            let hotkey = parse_shortcut("Ctrl+Win").expect("parse shortcut");
            let mut runtime = ShortcutRuntimeState::default();
            let mut pressed = HashSet::new();

            assert_eq!(
                send_key(
                    &mut runtime,
                    hotkey,
                    &mut pressed,
                    vk_value(VK_LCONTROL),
                    true
                ),
                ShortcutEventOutcome {
                    consume: false,
                    signal: None
                }
            );
            assert_eq!(
                send_key(&mut runtime, hotkey, &mut pressed, vk_value(VK_LWIN), true),
                ShortcutEventOutcome {
                    consume: true,
                    signal: Some(ShortcutSignal::Pressed)
                }
            );
            assert_eq!(
                send_key(&mut runtime, hotkey, &mut pressed, vk_value(VK_LWIN), false),
                ShortcutEventOutcome {
                    consume: true,
                    signal: Some(ShortcutSignal::Released)
                }
            );
            assert_eq!(
                send_key(
                    &mut runtime,
                    hotkey,
                    &mut pressed,
                    vk_value(VK_LCONTROL),
                    false
                ),
                ShortcutEventOutcome {
                    consume: false,
                    signal: None
                }
            );
        }

        #[test]
        fn keyed_ctrl_win_space_waits_for_space_before_pressing() {
            let hotkey = parse_shortcut("Ctrl+Win+Space").expect("parse shortcut");
            let mut runtime = ShortcutRuntimeState::default();
            let mut pressed = HashSet::new();

            assert_eq!(
                send_key(
                    &mut runtime,
                    hotkey,
                    &mut pressed,
                    vk_value(VK_LCONTROL),
                    true
                ),
                ShortcutEventOutcome {
                    consume: false,
                    signal: None
                }
            );
            assert_eq!(
                send_key(&mut runtime, hotkey, &mut pressed, vk_value(VK_LWIN), true),
                ShortcutEventOutcome {
                    consume: false,
                    signal: None
                }
            );
            assert_eq!(
                send_key(&mut runtime, hotkey, &mut pressed, vk_value(VK_SPACE), true),
                ShortcutEventOutcome {
                    consume: true,
                    signal: Some(ShortcutSignal::Pressed)
                }
            );
            assert_eq!(
                send_key(
                    &mut runtime,
                    hotkey,
                    &mut pressed,
                    vk_value(VK_SPACE),
                    false
                ),
                ShortcutEventOutcome {
                    consume: true,
                    signal: Some(ShortcutSignal::Released)
                }
            );
        }

        #[test]
        fn keyed_ctrl_alt_d_leaves_modifiers_live_until_d() {
            let hotkey = parse_shortcut("Ctrl+Alt+D").expect("parse shortcut");
            let mut runtime = ShortcutRuntimeState::default();
            let mut pressed = HashSet::new();

            assert!(
                !send_key(
                    &mut runtime,
                    hotkey,
                    &mut pressed,
                    vk_value(VK_LCONTROL),
                    true,
                )
                .consume
            );
            assert!(
                !send_key(&mut runtime, hotkey, &mut pressed, vk_value(VK_LMENU), true,).consume
            );
            assert_eq!(
                send_key(&mut runtime, hotkey, &mut pressed, vk_value(VK_D), true),
                ShortcutEventOutcome {
                    consume: true,
                    signal: Some(ShortcutSignal::Pressed)
                }
            );
            assert_eq!(
                send_key(&mut runtime, hotkey, &mut pressed, vk_value(VK_D), false),
                ShortcutEventOutcome {
                    consume: true,
                    signal: Some(ShortcutSignal::Released)
                }
            );
        }

        #[test]
        fn escape_cancels_active_modifier_shortcut() {
            let hotkey = parse_shortcut("Ctrl+Win").expect("parse shortcut");
            let mut runtime = ShortcutRuntimeState::default();
            let mut pressed = HashSet::new();

            let _ = send_key(
                &mut runtime,
                hotkey,
                &mut pressed,
                vk_value(VK_LCONTROL),
                true,
            );
            assert_eq!(
                send_key(&mut runtime, hotkey, &mut pressed, vk_value(VK_LWIN), true),
                ShortcutEventOutcome {
                    consume: true,
                    signal: Some(ShortcutSignal::Pressed)
                }
            );
            assert_eq!(
                send_key(
                    &mut runtime,
                    hotkey,
                    &mut pressed,
                    vk_value(VK_ESCAPE),
                    true
                ),
                ShortcutEventOutcome {
                    consume: true,
                    signal: Some(ShortcutSignal::Cancel)
                }
            );
            assert_eq!(
                send_key(&mut runtime, hotkey, &mut pressed, vk_value(VK_LWIN), false),
                ShortcutEventOutcome {
                    consume: false,
                    signal: None
                }
            );
        }

        fn send_key(
            runtime: &mut ShortcutRuntimeState,
            hotkey: super::ParsedShortcut,
            pressed: &mut HashSet<u32>,
            vk: u32,
            is_down: bool,
        ) -> ShortcutEventOutcome {
            let outcome = runtime.handle_event(hotkey, vk, is_down, !is_down, |candidate| {
                pressed.contains(&candidate)
            });
            // A low-level hook that consumes an event prevents Windows from
            // updating GetAsyncKeyState for that key.
            if !outcome.consume {
                if is_down {
                    pressed.insert(vk);
                } else {
                    pressed.remove(&vk);
                }
            }
            outcome
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ShortcutKey, parse_shortcut, shortcut_candidates, sync_runtime_pause_state};
    use parking_lot::Mutex;
    use std::sync::Arc;

    #[test]
    fn setup_registration_unpauses_the_runtime_hook_state() {
        let paused = Arc::new(Mutex::new(true));
        sync_runtime_pause_state(&paused, false);
        assert!(!*paused.lock());
    }

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
                "Ctrl+Win",
                "Ctrl+Win+Space",
                "Ctrl+Shift+Space",
                "Ctrl+Alt+D"
            ]
        );
    }

    #[test]
    fn parses_default_push_to_talk_hotkey() {
        let parsed = parse_shortcut("Ctrl+Win").expect("parse shortcut");
        assert!(parsed.ctrl);
        assert!(parsed.win);
        assert!(!parsed.alt);
        assert_eq!(parsed.key, None);
    }

    #[test]
    fn parses_keyed_fallback_hotkey() {
        let parsed = parse_shortcut("Ctrl+Win+Space").expect("parse shortcut");
        assert!(parsed.ctrl);
        assert!(parsed.win);
        assert_eq!(parsed.key, Some(ShortcutKey::Space));
    }
}
