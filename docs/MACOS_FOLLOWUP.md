# macOS Follow-Up

The prototype is Windows-first. A macOS pass should keep the same Tauri command contracts and replace platform-specific behavior behind services.

## Required Changes

- Replace Windows SendKeys paste helper with `Cmd+V` through a macOS accessibility-safe input path.
- Add microphone and accessibility permission education to onboarding.
- Convert tray-first behavior to menu bar conventions.
- Register the default shortcut as `Command+Option+Space` unless the user changes it.
- Add event tap handling for true press-and-hold dictation.

## Acceptance

- Dictation pastes into Notes, TextEdit, Safari textareas, Slack/Discord-style fields, and a terminal editor.
- Clipboard restore works after injection.
- Denied permissions produce actionable in-app states.
