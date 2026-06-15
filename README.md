# Atmospeak

Windows-first, local-only desktop dictation prototype built with Tauri 2, React, Rust, SQLite, and `whisper.cpp`.

## What Works

- Global shortcut event: `Ctrl+Win+Space` with `Ctrl+Alt+Space` fallback
- Tray menu: open app, start/stop dictation event, quit
- Microphone enumeration and recording through Rust/CPAL
- WAV output resampled to 16 kHz mono
- Bundled local `whisper-cli.exe`, required DLLs, and `ggml-base.en.bin`
- Deterministic cleanup for filler words, spoken punctuation, dictionary replacements, and snippets
- Clipboard paste injection through Windows SendKeys
- SQLite persistence for settings, history, dictionary, snippets, and stats
- Desktop onboarding for microphone selection, shortcut mode, privacy, and first paste test
- Browser mock mode for fast UI testing without Tauri

## First Run

```powershell
$env:PATH="$env:USERPROFILE\.cargo\bin;$env:PATH"
bun install
bun run tauri dev
```

No model path setup is required. Atmospeak resolves the bundled whisper.cpp
runtime and English base model from Tauri resources. Custom engine/model paths
are available only under **Advanced**.

## Verification

```powershell
bun run build
bun run test
$env:PATH="$env:USERPROFILE\.cargo\bin;$env:PATH"; cargo test --manifest-path src-tauri/Cargo.toml
```

## Release Build

```powershell
bun run release:build
bun run site:build
```

Release artifacts are written to `release/` and the static download site builds
to `dist-site/`. See [docs/RELEASE.md](docs/RELEASE.md) for updater signing,
GitHub Releases, checksums, and the unsigned Windows prototype limitation.

## Manual Acceptance

1. Start Notepad.
2. Focus the document body.
3. Hold `Ctrl+Win+Space` or the registered fallback.
4. Speak for at least one second.
5. Release the shortcut.
6. Confirm text is cleaned, pasted, saved to History, and clipboard restore behavior follows Settings.

## Prototype Boundaries

Atmospeak is clean-room software. It emulates the core desktop dictation workflow of modern voice input tools, but it does not copy proprietary UI, code, model services, names, assets, or private behavior.
