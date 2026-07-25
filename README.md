# Atmospeak

Windows-first, local-only desktop dictation built with Tauri 2, React, Rust, SQLite, and `whisper.cpp`.

**Version:** 0.2.0 (Phase A — Honest MVP). See [`docs/PHASE_A_HONEST_MVP.md`](docs/PHASE_A_HONEST_MVP.md).

## What works

- **DictationEngine (Rust)** owns the loop: idle → listening → processing → pasted/error
- Global hotkey (Windows low-level hook) and tray **dispatch into the engine** (not React)
- Push-to-talk and toggle modes (frozen mapping; toggle ignores key-up)
- CPAL microphone capture → 16 kHz mono WAV
- **Bundled `whisper-cli.exe`** (one process per utterance — **multi-second latency is normal**)
- Cleanup: fillers, spoken punctuation, dictionary, snippets, sentence casing
- Injection: clipboard + Ctrl+V with **last external window restore** and soft-fail “left on clipboard”
- SQLite settings/history/dictionary/snippets under `%LOCALAPPDATA%\Atmospeak`
- Stage metrics (log + events) for production validation
- Desktop onboarding (version `phase-a-honest-mvp-v1`)
- Browser mock mode for UI without Tauri

## What is *not* claimed

- Instant / sub-second cloud-style latency (CLI ASR cold-loads per utterance)
- Live streaming partials, AI polish, cloud STT, privacy auto-delete, export formats, FFT “pro” meter as product features
- Persistent Whisper host (Phase B — see [`docs/PHASE_B_ASR_HOST.md`](docs/PHASE_B_ASR_HOST.md))

## First run

Requires **Rust + MSVC Build Tools** (Desktop C++) and Bun:

```powershell
$env:PATH="$env:USERPROFILE\.cargo\bin;$env:PATH"
bun install
bun run tauri dev
```

## Verification

```powershell
bun run build
bun run test
bun run e2e
# Requires MSVC link.exe:
$env:PATH="$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo test --manifest-path src-tauri/Cargo.toml
```

## Manual acceptance (Phase A gate)

1. Start Notepad and focus the body.
2. Hold the registered shortcut (default family: `Ctrl+Win` / fallbacks).
3. Speak ≥1 second; release.
4. Expect cleaned text paste (or clipboard recovery message) and a History row.
5. Record stage metrics from Advanced / runtime events if validating 001–012.

Production matrix: `tests/manual/production-100.md` (hard pass **001** and **005**).

## Prototype boundaries

Atmospeak is clean-room software. It emulates the core desktop dictation workflow of modern voice input tools, but it does not copy proprietary UI, code, model services, names, assets, or private behavior.
