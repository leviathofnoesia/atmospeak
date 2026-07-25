# Changelog

## 0.3.0 — Phase B resident ASR host (2026-07-25)

### Added
- Resident `whisper-server.exe` (`services/asr_host.rs`) keeps the speech model warm
  instead of reloading it per utterance. Bundled from the same upstream archive as the
  CLI; see `docs/PHASE_B_ASR_HOST.md`.
- Automatic degradation to the one-shot CLI whenever the host is missing, disabled,
  slow to start, or failing mid-session. A dead host is respawned on the next utterance.
- `StageMetrics.asr_backend` now reports `"host"` or `"cli"` per utterance.
- `ATMOSPEAK_WHISPER_HOST=0` forces the CLI backend.
- Windows job object ties the server's lifetime to the app, so a crash or force-kill
  leaves no orphaned process holding the model.

### Fixed
- **Terminal phases never settled.** `tick_settle` only ran when a new command arrived,
  so the overlay stayed on `Pasted`/`Error` until the next user action. The engine worker
  now wakes itself on the settle deadline.
- **`handle_dictation_action` bypassed the mode table.** `"pressed"`/`"released"` mapped
  to unconditional start/stop, so overlay buttons behaved differently from the hotkey in
  toggle mode. They now route through the mode-aware arms (D10).
- Whisper subprocesses no longer flash a console window on every utterance
  (`CREATE_NO_WINDOW`).
- An empty transcript is reported as "no speech" rather than provoking a redundant
  CLI retry of audio the host already handled correctly.

### Tests
- Replaced the tautological `dictation_engine` placeholder with real coverage of the
  frozen transition table: one `Pressed` → one `Listening`, re-entry while `Processing`
  is ignored, toggle ignores key-up, mic-check exclusion, cancel guards, settle bounds.
  Rust suite: 16 → 25 tests.

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
