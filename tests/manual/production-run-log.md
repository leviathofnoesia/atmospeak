# Atmospeak Production Validation Run Log

This file tracks evidence for the 100-test production matrix in `tests/manual/production-100.md`.

## 2026-07-28 - 0.5.1 streaming recovery dogfood

- App version target: **0.5.1**
- Automated evidence:
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib`: **63 passed**
    (includes clamp math, drop-tolerance threshold, settle bounds).
  - `cargo test` in `src-asr-host`: **5 passed** (VAD window, overlap merge,
    PCM framing, VAD cadence).
  - `bun run test`: **24 passed**.
  - `scripts/verify-host-transcription.ps1`: **pass** in **1166 ms**
    (`The porcelain moon hums over the studio.`).
  - `scripts/build-asr-sidecars.ps1 -CpuOnly`: **pass** — rebuilt
    `atmospeak-asr-cpu.exe` with the reader/worker split.
  - Full `build-asr-sidecars.ps1` (Vulkan): **failed** in this environment
    during `whisper-rs-sys` cmake install with `VULKAN_SDK=C:\VulkanSDK\1.4.350.0`;
    prior `atmospeak-asr-vulkan.exe` remains from 0.5.0 and must be rebuilt
    before Vulkan dogfood.
- Code-level acceptance for the three regressions:
  - Stop path sends `StopSession` after writer flush and awaits final while
    teardown runs; micro-drops ≤12 frames stay on the streamed path.
  - `ShowWindow(SW_RESTORE)` is gated behind `IsIconic` — maximized targets
    are never resized by paste restore.
  - `save_overlay_position` clamps before persist and returns `(x, y)`; the
    overlay settles via `setPosition` when the OS left it off-screen.
- Operator checklist still required for live latency/geometry (needs mic + UI):
  - Stop→paste on **3 s / 30 s / 120 s** clips with CPU `base.en`
    (target: ≤≈1 s after release on the 3 s clip).
  - Same matrix on Vulkan once `VULKAN_SDK` is available and
    `build-asr-sidecars.ps1` rebuilds `atmospeak-asr-vulkan.exe`.
  - Paste into **maximized** Notepad / VS Code / Chrome — geometry unchanged.
  - Drag the orb past every screen edge — springs back and persists across
    restart.

## 2026-07-26 - 0.3.1 recovery candidate (not approved for publication)

- Public containment:
  - v0.3.0 release and tag withdrawn; source commit and local artifact hashes preserved.
  - Website download CTAs replaced by the temporary repair notice.
  - Direct v0.3.0 artifact and `latest.json` URLs return 404.
- Automated recovery evidence:
  - `cargo test --manifest-path src-tauri/Cargo.toml`: **46 passed**
  - `bun run test`: **19 passed**
  - `bun run e2e`: **10 passed**, including canonical 1000x660 Home/History
    screenshots with a maximum two-pixel difference, 125%/150% scaling, 360px
    overflow, setup-without-skip, nested diagnostics, editable hub records, and
    a motionless idle overlay.
  - `bun run build` and `bun run site:build`: **pass**
  - `bun run release:build`: **pass**, producing the 0.3.1 NSIS installer,
    MSI, portable ZIP, signatures, checksums, and updater metadata locally.
  - `bun run release:test-install`: **pass**; the installed release candidate
    exposed one setup page with Welcome visible and no overlay, then uninstalled.
  - Native fresh-profile WebView2 inspection: exactly one setup WebView, Welcome
    visible, no overlay, no setup skip.
  - Native completed-profile inspection: exactly one overlay WebView, no idle
    canvas or running Web animation.
  - Native steady-state release overlay CPU: **0% process-tree delta over 30s**
    in the observed sample; hidden debug overlay: **0% over 10s**.
  - Native move/restart check: `(140,160)` persisted exactly; deliberate reset
    removed the saved-position file without the move event recreating it.
  - The first shortcut repair was rejected after a physical `Ctrl+Win` attempt
    remained stuck in “Listening for keys.” Its `Ctrl+Alt+D` probe did not prove
    the selected default and is not treated as acceptance evidence.
  - Replacement native shortcut harness: the rendered setup page recorded
    `Ctrl+Alt+K`, lit `Ctrl`, `Alt`, and `K` independently while held, required
    the same chord again, and unlocked Continue only after native
    `pressed`/`released` events. The same fresh native process also registered
    and exercised exact `Ctrl+Shift+F12` and modifier-only `Ctrl+Win` chords.
  - Follow-up responsiveness repair moved chord assembly out of React and into
    the native hook, with UI events dispatched off the low-level hook thread.
    A fresh native run painted `Ctrl`, then `Ctrl+Alt`, then `Ctrl+Alt+K` after
    each individual key-down; peak measured native-event delivery was **3 ms**,
    the WebView console was clean, and release completed `Ctrl+Alt+K` once.
  - Focus-specific retest exposed that the global hook path behaved differently
    while the onboarding WebView owned focus. Recording now uses focused
    `keydown`/`keyup` events with synchronous visual updates; only “Test
    selected” uses the native global hook. A native focused-WebView run painted
    `Ctrl`, `Ctrl+Alt`, and `Ctrl+Alt+K` at **46 / 18 / 16 ms**, completed the
    chord on release, then passed the separate global test with a clean console.
  - The runtime no longer substitutes a preset when the requested shortcut is
    invalid or unavailable; the selected normalized chord is the registered
    chord or setup reports a failure.
- Real hardware discovery:
  - `Microphone (Elgato Wave:3)` was enumerated by the native app.
  - Ambient input measured about -47.8 dBFS RMS and correctly failed the speech
    gate. This is not a substitute for deliberate speech.
- Existing profile preservation audit:
  - 3 transcript rows and all 3 referenced WAV files remain present.
  - Only the additive `source_application` database column was migrated.
  - Retention defaults to **Keep until deleted** for legacy and new profiles.
- **Human gates still pending:** speak the required porcelain-moon phrase through
  the Elgato Wave:3, then run production tests 001 and 005. Record session IDs,
  transcript/output, `capture_ms`, `asr_ms`, `total_ms`, RMS, peak, SNR, and
  `asr_backend: host`. The candidate remains a draft until both pass.

## 2026-07-26 - 0.3.0 daily-driver and release shell

- Automated verification (no microphone involved):
  - `cargo test --manifest-path src-tauri/Cargo.toml`: **32 passed**
  - `bun run build`: **pass**
  - `bun run test`: **11 passed**
  - `bun run site:build`: **pass**, including the website TypeScript check
  - Downloader coverage includes managed-model path resolution, checksum rejection
    without damaging an installed model, and verified atomic replacement.
- Browser QA:
  - Landing page checked at 1280 px and 360 px widths.
  - Version-derived release filenames, install docs, SmartScreen instructions,
    shared assets/tokens, and responsive stacking render correctly.
- **Operator gate remains pending:** run 001 and 005 with deliberate speech and
  record the real session id, target output, and Advanced → Last stage metrics.
  Confirm `asr_backend: host` and compare that run's `asr_ms` with the synthetic
  CLI/host timings below. No host claim is promoted from synthetic evidence.

## 2026-07-25 - Phase B 0.3.0 resident ASR host

- Automated verification (no microphone involved):
  - `cargo test --manifest-path src-tauri/Cargo.toml`: **25 passed** (was 16)
  - `bun run build`, `bun run test` (8 passed): pass
  - `whisper-server.exe` confirmed present in upstream `whisper-bin-x64.zip` v1.8.4
  - Host lifecycle in the real app: launched `atmospeak.exe`, confirmed a
    `whisper-server` process resident at ~206 MB (model loaded); hard-killed
    `atmospeak.exe` and confirmed **0 orphaned** `whisper-server` processes
- Backend comparison on 6.42 s of **synthesized** speech (`base.en`, CPU), which is a
  plumbing check, not a product measurement:

  | Backend | ASR time (3 runs) |
  |---|---|
  | `cli` (cold model each run) | 2.57 / 2.62 / 1.98 s |
  | `host` (warm) | 1.93 / 1.82 / 1.55 s |

  Model load ≈ 0.7 s per utterance is what the host removes. Transcript identical on
  both paths.
- **Still operator-pending — the actual gate.** 001 and 005 with a real microphone,
  recording session id, audio path, target-app output, and `asr_backend` per run.
  Nothing above substitutes for this.

## 2026-07-25 - Phase A 0.2.0 implementation

- App version target: **0.2.0**
- Design: `docs/PHASE_A_HONEST_MVP.md` implemented (DictationEngine, contract lock, injection restore, metrics, app data migrate, honest UI).
- ASR backend label: `cli` (stock whisper-cli per utterance).
- Automated: frontend `bun run build` / `bun run test` re-verified after honesty pass.
- **MSVC Build Tools installed** — native verification completed:
  - `cargo test --manifest-path src-tauri/Cargo.toml`: **16 passed**
  - `cargo build --manifest-path src-tauri/Cargo.toml`: **pass**
  - Compile fixes: `IsWindow(Some(hwnd))` for windows-0.61; `use tauri::Manager` for `try_state` in metrics
- Mic evidence for 001–012 still pending operator dogfood (`bun run tauri dev`).
- Hard gate remains: **001** and **005** with session id, audio path, stage metrics (`capture_stop_ms`, `write_ms`, `asr_ms`, `cleanup_ms`, `inject_ms`, `total_ms`).

## 2026-06-16 - Baseline Automation And Matrix Setup

- Matrix status: created `tests/manual/production-100.md` with exactly 100 numbered production validation cases.
- Audio fixture search: no committed `.wav`, `.mp3`, `.m4a`, `.flac`, or `.ogg` files found under `src`, `src-tauri`, `tests`, or `docs`.
- Current real-recording status: not started. The next required step is to run `bun run tauri dev` on a machine with microphone access and execute test IDs 001-012 first to establish real dictation baseline quality.
- Automated evidence:
  - `bun run build`: pass.
  - `bun run test`: pass, with expected jsdom warning for missing canvas `getContext`.
  - `bun run e2e`: pass.
  - `$env:USERPROFILE\.cargo\bin\cargo.exe test` from `src-tauri`: pass, 15 Rust tests.
  - `bun run validation:production`: pass, matrix has 100 cases, 0 started runs, 0 completed runs, and 100 pending cases.
  - `bun run validation:production -- -NewRunIds 001,002`: pass, prints ready-to-fill run templates for selected IDs.
- Finding fixed during setup:
  - `src-tauri/src/services/overlay_window.rs` used `420x128` for show/reset while `tauri.conf.json` configures the overlay as `520x150`.
  - Updated show/reset constants to `520x150` so production overlay positioning matches the real transparent window size.

## Next Real-Recording Batch

## 2026-07-27 - Native push-to-talk release automation

- Automated native fixture gate: **pass twice consecutively**
- Custom Settings shortcut: `Ctrl+Alt+F12`
- Capture mode: push-to-talk
- Release behavior: stopped capture, transcribed with `asr_backend: host`, and
  pasted exactly once into a native Windows text box
- Session ids: `4b4f14bf-2d56-416f-8508-a324f4dd1a1b`,
  `02659d39-811a-401a-b509-7f094508f338`
- Transcript/target output: `The porcelain moon hums over the studio.`
- Stage metrics: `capture_stop_ms=0`, `asr_ms=895/850`,
  `inject_ms=43/44`, `total_ms=989/947`
- Sound-check metrics: 3244 ms capture, 1800 ms active speech,
  -28.19 dBFS RMS, -10.93 dBFS peak, 18.70 dB SNR, 0% clipping,
  token similarity 1.0, resident host ASR
- Scope: isolated debug profile and deterministic fixture. This proves the
  native key-release-to-paste path, captured-target restoration, and arbitrary
  Settings hotkey wiring. It does **not** satisfy production tests 001 or 005;
  both remain pending with real Elgato Wave:3 speech.

### Shortcut persistence regression

- Reproduced the feedback-only failure by leaving native shortcut capture and
  test modes armed before saving a custom chord.
- After the fix, Save cleared both transient modes, persisted
  `Ctrl+Alt+F12`, updated the native orb label, started listening from that
  exact chord, and injected once on release.
- Verified twice consecutively with native session ids
  `0c89f210-a4fa-4dc5-bbd7-f6e8c5873458` and
  `e7069c1d-cca8-4ad5-9ccf-322c8c99f936`.
- Host ASR: 941/996 ms; total release-to-result pipeline: 1163/1222 ms.

### Installed Ctrl+CapsLock regression

- Found and removed an installer-smoke Start-menu shortcut and registry entry
  that incorrectly targeted `AtmospeakInstallTest` under `%TEMP%`.
- Restored the real installation and Start-menu target under
  `%LOCALAPPDATA%\Atmospeak`.
- Verified the persisted production chord `Ctrl+CapsLock` twice through the
  complete fixture pipeline, including release-to-host-ASR and exactly one
  native paste. Session ids: `7094c139-32f2-403e-bc13-b74bfd23cfb9` and
  `ff6c3d5d-b14b-4972-99e1-94f802470bba`.
- Verified the rebuilt installed release with an external native editor
  focused: Ctrl lead-key feedback was visible globally, CapsLock produced the
  native listening event, and the orb rendered `data-state=listening`.
- Replaced the dock's focus-local WebView keyboard animation with registered
  native shortcut key-edge events. The installer harness now refuses to
  overwrite a real installation and cleans test-only registry/shortcut state.

### Installed v0.4.0 elevated-Terminal regression

- Confirmed Windows Terminal PID `21832` was elevated while Atmospeak ran at
  normal user integrity, reproducing the security boundary that hid physical
  keys from the former low-level runtime hook.
- Replaced keyed runtime activation with a Windows system-registered hotkey;
  push-to-talk release is detected by bounded key-state monitoring.
- Verified the packaged and installed `0.4.0` executable from
  `%LOCALAPPDATA%\Atmospeak` with the real elevated Windows Terminal focused:
  `Ctrl+CapsLock` produced the native listening event and the orb entered
  `data-state=listening`. The probe cancelled without transcription or
  injection so it did not alter the terminal session.
- Re-ran the exact `Ctrl+CapsLock` fixture pipeline through release-triggered
  host ASR and one native paste. Session id
  `1e928be6-b0bf-498a-b4ad-78105711ae38`; ASR `1008 ms`; total `1102 ms`.

Run these first because they establish whether the core production loop is healthy before spending time on polish, update, and destructive recovery cases:

1. `001` - Notepad one-shot sentence.
2. `002` - Browser text field one-shot.
3. `003` - IDE/editor comment dictation.
4. `004` - Word/rich text dictation.
5. `005` - Push-to-talk short phrase.
6. `006` - Push-to-talk three-turn sequence.
7. `007` - Toggle mode paragraph.
8. `008` - 30+ second long-form dictation.
9. `009` - Under-2-second short command.
10. `010` - Fast speech.
11. `011` - Slow speech with long pauses.
12. `012` - Accent or non-native English phrasing.

Do not mark any of these complete without a real Atmospeak session id/audio path and a target app output check.

To append ready-to-fill entries for the whole first batch:

```powershell
bun run validation:production -- -NewRunIds 001,002,003,004,005,006,007,008,009,010,011,012 -AppendTemplates
```

To check progress after any run:

```powershell
bun run validation:production -- -ListPending
```

## Real Recording Entry Template

```markdown
### RUN-YYYYMMDD-HHMM - Test ID

- Status:
- Environment:
- App version/commit:
- Target app:
- Settings changed:
- Audio/session id:
- Audio path:
- Raw transcript:
- Cleaned transcript:
- Target output:
- Notices/runtime events:
- Latency/performance:
- Accuracy:
- Correct feature use:
- Conciseness/completeness:
- Recovery:
- UI/UX smoothness:
- Performance:
- Issues found:
- Fix applied:
- Retest link:
```
