# Changelog

## 0.2.0 — Phase A Honest MVP (2026-07-25)

### Added
- Rust `DictationEngine` actor (`services/dictation_engine.rs`) owns the dictation state machine.
- Hotkey (Windows LL hook) and tray route through `dispatch_fire_and_forget` (single path).
- Blocking IPC `start_recording` / `stop_recording` remain engine-backed with existing return shapes.
- Commands: `handle_dictation_action`, `mic_check_start` / `mic_check_stop`, `set_shortcut_test_active`, `get_runtime_events`, `get_last_stage_metrics`, `show_main_window`.
- Injection: last external HWND restore, soft-fail leaves transcript on clipboard, expanded `InjectionResult`.
- Stage metrics (`capture_stop_ms`, `write_ms`, `asr_ms`, `cleanup_ms`, `inject_ms`, `total_ms`, `asr_backend: "cli"`).
- App data directory `%LOCALAPPDATA%\Atmospeak` with one-way migrate from `Wind Speak` (DB filename kept).
- Onboarding version unified: `phase-a-honest-mvp-v1` (Rust + TS).
- Docs: `docs/PHASE_A_HONEST_MVP.md`, `docs/PHASE_B_ASR_HOST.md`.

### Changed
- Frontend `AppSettings` locked to the 12 Rust fields; phantom polish/privacy/export/FFT/language UI removed.
- Startup Run key renamed to `Atmospeak` (legacy `Wind Speak` removed on update).
- README honesty: CLI latency is multi-second; no false streaming/polish claims.

### Notes
- Persistent Whisper host is **not** shipped (stock `whisper-cli` is one-shot only). See Phase B doc.
- Production mic matrix 001–012 still requires operator evidence on a machine with MSVC + mic.

## Unreleased (historical pre-0.2.0 draft notes)

Earlier CHANGELOG “Unreleased” bullets describing polish/FFT/privacy as shipped were **frontend/docs ahead of backend** and are superseded by 0.2.0 honesty.
