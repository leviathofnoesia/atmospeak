# TODO � Atmospeak Feature Parity

> **Phase A (2026-07-25):** Honesty pass. Many items below that previously appeared �done� were frontend-only or non-existent in Rust. See `docs/PHASE_A_HONEST_MVP.md`. Treat this file as aspirational after Phase A engine work, not as a completion ledger for pre-A1 claims.

Each ticket is sized for a single PR. Sources: WF = Wispr Flow, BV = BridgeVoice, CC = cross-cutting. Full plan lives in `paritygoal.md`.

## Phase A delivered (honest MVP)

- [x] Contract lock (TS AppSettings = 12 Rust fields)
- [x] DictationEngine actor owns loop; shortcuts/tray dispatch in Rust
- [x] CLI ASR path documented (not snappy; multi-second expected)
- [x] Injection last-target restore + soft-fail clipboard
- [x] Stage metrics emit/log
- [x] App data migrate to `%LOCALAPPDATA%\Atmospeak`
- [x] UI stripped of false polish/privacy/export/FFT claims
- [x] Version 0.2.0 scaffold

## Phase B delivered (resident ASR host, 0.3.0)

- [x] Bundled `whisper-server.exe`; model stays warm between utterances
- [x] Automatic CLI fallback + respawn; `ATMOSPEAK_WHISPER_HOST=0` kill-switch
- [x] `asr_backend` label on stage metrics (`host` / `cli`)
- [x] Job object so the host never outlives the app
- [x] Fixed: terminal phases never settled back to idle
- [x] Fixed: `handle_dictation_action` bypassed the PTT/toggle mode table
- [x] Fixed: console window flash per utterance
- [x] Real `dictation_engine` transition tests (16 → 25 Rust tests)

## Blocking everything else

- [ ] **Production 001 + 005 with a real microphone.** No dictation session has ever
      completed end to end; `transcript_sessions` is still empty. Until this passes,
      every "works" claim above rests on unit tests and process inspection only.

## Remaining (post Phase A)

See `docs/PHASE_A_HONEST_MVP.md` non-goals and prior Tier 1/2 lists in git history / `paritygoal.md`.
