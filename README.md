<p align="center">
  <img src="src/assets/nov-pax/assets/brand/logo-lotus-eye.png" width="112" alt="Atmospeak lotus-eye mark">
</p>

<h1 align="center">Atmospeak</h1>

<p align="center">
  Streaming, local-first voice dictation for Windows. Speak naturally, preview locally, and paste once.
</p>

<p align="center">
  <a href="https://github.com/leviathofnoesia/atmospeak/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/leviathofnoesia/atmospeak?style=flat-square&color=5969a6"></a>
  <a href="https://github.com/leviathofnoesia/atmospeak/releases/latest/download/atmospeak_0.5.0_x64-setup.exe"><img alt="Windows x64" src="https://img.shields.io/badge/Windows-x64-171720?style=flat-square"></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-79966f?style=flat-square"></a>
</p>

<p align="center">
  <a href="https://leviathofnoesia.github.io/atmospeak/"><strong>Website</strong></a>
  ·
  <a href="https://github.com/leviathofnoesia/atmospeak/releases/latest/download/atmospeak_0.5.0_x64-setup.exe"><strong>Download v0.5.0</strong></a>
  ·
  <a href="https://github.com/leviathofnoesia/atmospeak/releases/latest"><strong>Release notes</strong></a>
</p>

Atmospeak is a Windows desktop dictation app built around a local
[`whisper.cpp`](https://github.com/ggerganov/whisper.cpp) runtime. It requires
no account and sends no microphone audio to a transcription service.

## What ships in 0.5.0

- **Streaming local transcription.** Atmospeak decodes bounded speech segments
  while the microphone is still active, then reconciles only the remaining tail
  after you stop.
- **Live dock preview, one final paste.** Partial and stable text stays inside
  the Atmospeak dock. The target application receives exactly one final paste.
- **Vulkan acceleration with CPU fallback.** A crash-isolated ASR sidecar tries
  the local Vulkan backend first and falls back through CPU, the resident batch
  host, and the one-shot CLI.
- **Automatic Balanced model selection.** Atmospeak measures local backlog and
  finalization time, then chooses among models you already installed. It never
  downloads or deletes a model automatically.
- **Reliable Hold and Tap gestures.** Hold stops as soon as any required chord
  key is released. Tap stops on the second complete press without depending on
  release polling.
- **Local VAD and bounded decoding.** Silero VAD finalizes natural speech
  segments, force-splits continuous speech, and keeps long dictations from
  requiring a complete re-transcription at stop.
- **Resilient fallback.** Full audio remains available locally if streaming
  fails, and fallback transcription still produces only one final result.
- **Inspectable local diagnostics.** Shortcut acknowledgements, ASR timing,
  backlog, dropped-frame counts, and fallback reasons are stored locally
  without dictated text or unrelated keystrokes.

## Install

Atmospeak currently supports **Windows 10/11 x64**.

| Package | Use it when | Download |
| --- | --- | --- |
| Setup EXE | Recommended installation | [atmospeak_0.5.0_x64-setup.exe](https://github.com/leviathofnoesia/atmospeak/releases/latest/download/atmospeak_0.5.0_x64-setup.exe) |
| MSI | Managed or MSI-based deployment | [atmospeak_0.5.0_x64_en-US.msi](https://github.com/leviathofnoesia/atmospeak/releases/latest/download/atmospeak_0.5.0_x64_en-US.msi) |
| Portable ZIP | Run without a system-wide install | [atmospeak_0.5.0_x64-portable.zip](https://github.com/leviathofnoesia/atmospeak/releases/latest/download/atmospeak_0.5.0_x64-portable.zip) |
| Checksums | Verify downloaded artifacts | [SHA256SUMS.txt](https://github.com/leviathofnoesia/atmospeak/releases/latest/download/SHA256SUMS.txt) |

The Windows installers are not Authenticode-signed yet. SmartScreen may show
**Windows protected your PC**. Choose **More info**, verify the app name is
Atmospeak, and choose **Run anyway**. You can verify the download against the
published SHA-256 manifest before opening it.

## Daily use

1. Complete the six-step first-run setup and its real microphone transcription.
2. Leave Atmospeak in the tray.
3. Choose **Hold** or **Tap** in Settings.
4. In Hold mode, hold the default `Ctrl+Win` shortcut, speak, then release any
   chord key. In Tap mode, press the chord once to start and again to stop.
5. Watch the local preview in the dock while speaking. Atmospeak finalizes the
   remaining tail and pastes once at the cursor that was active when recording
   began.

If Windows prevents a normal
process from injecting into a higher-integrity application, Atmospeak leaves
the transcript on the clipboard instead of discarding it.

Local application data lives under:

```text
%LOCALAPPDATA%\Atmospeak
```

That directory contains settings, history, downloaded models, diagnostics, and
the local database. Atmospeak does not require a cloud account.

## Speech models

The bundled model works immediately. Optional models download from their
published Hugging Face repositories into Atmospeak's local model directory,
stream to a temporary file, and are installed only after their pinned SHA-256
hash passes.

| Atmospeak ID | Upstream model | Availability | Approx. size |
| --- | --- | --- | ---: |
| `tiny.en` | Whisper Tiny English | Setup / Settings | 75 MB |
| `base.en` | Whisper Base English | **Bundled default** | 142 MB |
| `small.en` | Whisper Small English | Setup / Settings | 466 MB |
| `medium.en` | Whisper Medium English | Settings | 1.43 GB |
| `large-v3-turbo-q5` | Whisper Large v3 Turbo q5 | Settings | 548 MB |
| `distil-large-v3.5` | Distil-Whisper Large v3.5 | Settings | 1.42 GB |
| `distil-large-v3` | Distil-Whisper Large v3 | Legacy installs | 1.42 GB |

If an optional selection is missing, Atmospeak falls back to the bundled
`base.en` model. Advanced Settings can also point to a custom compatible
Whisper CLI and GGML model.

## How it works

```text
Hold / Tap shortcut or dock
        ↓
Rust dictation engine → immediate stop acknowledgement
        ↓
CPAL capture → bounded queue → mono 16 kHz frames
        ↓
Silero VAD → rolling preview + stable Whisper segments
        ↓
Vulkan sidecar → CPU sidecar → resident host → CLI fallback
        ↓
Tail reconciliation → cleanup + dictionary + snippets exactly once
        ↓
Restore original target → one clipboard paste → local History
```

Ordinary dictation is rejected before ASR when it is effectively silent, too
quiet, or too noisy. This prevents empty recordings from becoming Whisper
hallucinations.

## Build from source

Requirements:

- Windows 10/11 x64
- Rust toolchain
- Visual Studio Build Tools with **Desktop development with C++**
- [Bun](https://bun.sh/)
- [Git LFS](https://git-lfs.com/)

```powershell
git clone https://github.com/leviathofnoesia/atmospeak.git
cd atmospeak
git lfs install
git lfs pull
bun install
bun run tauri dev
```

The bundled GGML model and `whisper.cpp` binaries are LFS-tracked. If a clone
contains pointer files instead of the runtime, recover them with:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/bootstrap-whisper.ps1
```

## Validate a change

```powershell
bun run build
bun run test
bun run e2e
bun run site:build
cargo test --manifest-path src-tauri/Cargo.toml
```

The native push-to-talk harness exercises shortcut persistence, key-down,
key-up, resident-host transcription, target restoration, and exactly one
native paste:

```powershell
bun run validation:native-ptt
```

The deterministic audio seam exists only in debug builds. Release acceptance
still requires real microphone runs for Notepad one-shot (`001`) and
push-to-talk (`005`), recorded in
[`tests/manual/production-run-log.md`](tests/manual/production-run-log.md).

## Current boundaries

- Windows x64 only.
- Unsigned installers; SmartScreen warnings remain.
- No cloud STT, mobile client, cross-device synchronization, or live typing
  into the target application.
- Live preview is local and appears only in the Atmospeak dock.
- Latency depends on speech length, selected model, GPU/CPU performance, and
  current ASR backlog. Atmospeak exposes measured timings rather than promising
  a fixed latency on every machine.
- Updating downloads the complete installer; Tauri does not provide delta
  updates here.

## Project

Atmospeak uses Tauri 2, React, TypeScript, Rust, SQLite, CPAL, WebView2, and
`whisper.cpp`. It is clean-room software and does not contain proprietary code,
assets, or model services from competing dictation products.

See [`docs/RELEASE.md`](docs/RELEASE.md) for packaging and updater details,
[`docs/MODEL_BOOTSTRAP.md`](docs/MODEL_BOOTSTRAP.md) for runtime recovery, and
[`tests/manual/production-100.md`](tests/manual/production-100.md) for the
manual production matrix.

Atmospeak source code is available under the [MIT License](LICENSE). Bundled
third-party notices are reproduced in
[`src-tauri/resources/ACKNOWLEDGEMENTS.md`](src-tauri/resources/ACKNOWLEDGEMENTS.md).
