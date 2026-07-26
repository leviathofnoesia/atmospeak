# Atmospeak Production Validation Run Log

This file tracks evidence for the 100-test production matrix in `tests/manual/production-100.md`.

## 2026-07-26 - 0.3.1 recovery candidate (not approved for publication)

- Public containment:
  - v0.3.0 release and tag withdrawn; source commit and local artifact hashes preserved.
  - Website download CTAs replaced by the temporary repair notice.
  - Direct v0.3.0 artifact and `latest.json` URLs return 404.
- Automated recovery evidence:
  - `cargo test --manifest-path src-tauri/Cargo.toml`: **43 passed**
  - `bun run test`: **18 passed**
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
  - Native setup shortcut probe: an OS-level injected `Ctrl+Alt+D` produced the
    expected `pressed` then `released` Tauri events without entering dictation.
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
