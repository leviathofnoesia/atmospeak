# Atmospeak Production Validation Run Log

This file tracks evidence for the 100-test production matrix in `tests/manual/production-100.md`.

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
