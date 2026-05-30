use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use std::{collections::HashSet, sync::Arc};
use tauri::{AppHandle, Emitter, Runtime};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

use crate::models::ShortcutStatus;

#[derive(Clone)]
struct ShortcutCandidate {
    label: String,
    shortcut: Shortcut,
}

pub fn register_shortcut<R: Runtime>(
    app: &AppHandle<R>,
    shortcut_status: Arc<Mutex<ShortcutStatus>>,
    requested_hotkey: &str,
    paused: bool,
) -> ShortcutStatus {
    #[cfg(desktop)]
    {
        let _ = app.global_shortcut().unregister_all();
        let candidates = shortcut_candidates(requested_hotkey);
        let mut failures = Vec::new();

        for candidate in candidates {
            match app.global_shortcut().register(candidate.shortcut) {
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
                        message: format!("Global shortcut registered: {}.{}", candidate.label, fallback_note),
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
        return status;
    }

    #[allow(unreachable_code)]
    {
        let status = ShortcutStatus {
            registered: false,
            hotkey: String::new(),
            paused,
            message: "Global shortcuts are unavailable on this platform build.".to_string(),
        };
        *shortcut_status.lock() = status.clone();
        status
    }
}

pub fn set_paused<R: Runtime>(
    app: &AppHandle<R>,
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
        format!("Global shortcut registered: {}.", status.hotkey)
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
            if !seen.insert(normalized.clone()) {
                return None;
            }
            parse_shortcut(&normalized)
                .ok()
                .map(|shortcut| ShortcutCandidate { label: normalized, shortcut })
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
        return Err(anyhow!("shortcut must include at least one modifier and one key"));
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

fn parse_shortcut(label: &str) -> Result<Shortcut> {
    let mut modifiers = Modifiers::empty();
    let mut key = None;
    for part in label.split('+') {
        match part {
            "Alt" => modifiers |= Modifiers::ALT,
            "Ctrl" => modifiers |= Modifiers::CONTROL,
            "Shift" => modifiers |= Modifiers::SHIFT,
            "Win" => modifiers |= Modifiers::SUPER,
            "Space" => key = Some(Code::Space),
            "D" => key = Some(Code::KeyD),
            _ => return Err(anyhow!("unsupported shortcut part: {part}")),
        }
    }
    key.map(|code| Shortcut::new(Some(modifiers), code))
        .ok_or_else(|| anyhow!("shortcut must include a key"))
}

#[cfg(test)]
mod tests {
    use super::shortcut_candidates;

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
}
