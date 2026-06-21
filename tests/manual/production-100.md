# Atmospeak 100-Test Production Validation Matrix

Use this matrix for real-world validation with production-like data. Every test that involves dictation must use a real microphone recording made through Atmospeak, not a typed mock transcript. Keep the audio path, raw transcript, cleaned transcript, target app output, notice text, runtime events, latency notes, and any screenshots/logs in the execution log.

## Scoring Rubric

Score each completed test on these axes:

- Accuracy: transcript, cleanup, polish, export, or action output is correct.
- Correct feature use: expected pipeline stages fired, such as cleanup, snippets, shortcuts, injection, history, or polish.
- Conciseness and completeness: notices and outputs are brief but sufficient.
- Recovery: errors, edge cases, and unexpected states are handled without data loss or confusion.
- UI/UX smoothness: visual feedback, overlay state, transitions, and notices are clear.
- Performance: startup, recording start, transcription latency, polish latency, injection speed, and UI responsiveness are acceptable.

Suggested rating: `pass`, `minor issue`, `fail`, or `blocked`, with a short note and a fix/retest link when needed.

## Production Data Setup

- Use at least four target apps: Notepad, a browser text field, an IDE/editor, and Word or another rich text editor.
- Use at least three voices or speaking styles when possible: normal pace, fast pace, and accented or non-native phrasing.
- Capture short, medium, and long recordings: under 3 seconds, 5-15 seconds, and 30+ seconds.
- Before destructive tests, export or back up the active Atmospeak profile/database.
- For external AI tests, record provider, endpoint, model, and whether the API key was present only in the desktop environment.

## Test Matrix

| ID | Area | Production scenario | Data to capture | Pass criteria |
|---:|---|---|---|---|
| 001 | Dictation | One-shot recording into Notepad with a single complete sentence. | Audio, raw text, cleaned text, Notepad output, elapsed time. | Text is accurate, saved to history, and injected once. |
| 002 | Dictation | One-shot recording into a browser search box. | Target URL, audio, output text, injection result. | Focused browser field receives the transcript without extra characters. |
| 003 | Dictation | One-shot recording into an IDE editor comment block. | IDE name, file type, raw/cleaned text, output text. | Transcript preserves developer terms well enough for practical use. |
| 004 | Dictation | One-shot recording into Word or rich text editor body. | App name, output formatting, raw/cleaned text. | Text lands at cursor and editor remains usable. |
| 005 | Dictation | Push-to-talk short phrase with press and release. | Shortcut mode, audio duration, state transitions. | Recording starts on press, stops on release, no stuck listening state. |
| 006 | Dictation | Push-to-talk multi-turn sequence of three separate phrases. | Three session ids, target app output, history order. | Each turn creates one session and output order matches speech order. |
| 007 | Dictation | Toggle mode start and stop for a medium paragraph. | Hotkey, notice sequence, raw/cleaned text. | Toggle starts/stops reliably and no audio continues after stop. |
| 008 | Dictation | Long-form 30+ second dictation with several sentences. | Audio length, raw/cleaned text, transcription latency. | Full text is captured without truncation; latency is acceptable. |
| 009 | Dictation | Very short command under 2 seconds, such as "save draft". | Audio length, notice, history record. | App handles short input clearly, with either usable transcript or clear too-short recovery. |
| 010 | Dictation | Fast speech with run-on phrasing. | Audio, raw/cleaned text, accuracy score. | Important words are retained and cleanup does not distort meaning. |
| 011 | Dictation | Slow speech with long pauses. | Audio, session duration, transcript gaps. | Pauses do not prematurely end or duplicate text. |
| 012 | Dictation | Accent or non-native English phrasing. | Speaker note, audio, raw/cleaned text. | Transcript is usable and errors are understandable, not catastrophic. |
| 013 | Cleanup | Spoken punctuation: comma, period, question mark, exclamation mark. | Raw text, cleaned text. | Spoken punctuation becomes correct punctuation with spacing. |
| 014 | Cleanup | Paragraph command: "new paragraph" between two thoughts. | Raw text, cleaned text, injected output. | Cleaned output contains a paragraph break and sentence casing. |
| 015 | Cleanup | Line command: "new line" in a short list. | Raw text, cleaned text, target output. | Output contains intended line break, not literal words. |
| 016 | Cleanup | Filler words: "um", "uh", "erm", "ah". | Raw text, cleaned text. | Fillers are removed without removing meaningful neighboring words. |
| 017 | Cleanup | Filler word "like" used as filler. | Raw text, cleaned text. | Filler "like" is removed when filler-like, and result remains grammatical enough. |
| 018 | Cleanup | Correction command: "scratch that" followed by replacement phrase. | Raw text, cleaned text. | Text before the correction command is removed. |
| 019 | Cleanup | Correction command: "never mind" after a false start. | Raw text, cleaned text. | Text before "never mind" is removed and only revised phrase remains. |
| 020 | Cleanup | Dictionary replacement for product name, such as "wind speak" to "Atmospeak". | Dictionary entry, raw/cleaned text. | Enabled replacement applies case-insensitively. |
| 021 | Cleanup | Disabled dictionary entry during dictation. | Entry state, raw/cleaned text. | Disabled entry does not apply. |
| 022 | Cleanup | Snippet trigger phrase in live preview and final text. | Snippet, overlay preview, cleaned text. | Snippet expands once and does not double-expand. |
| 023 | Cleanup | Whisper timestamp stripping from transcript-like text. | Raw text containing timestamp, cleaned text. | Timestamp markers are removed cleanly. |
| 024 | Cleanup | Sentence casing after punctuation and paragraph breaks. | Raw text, cleaned text. | First letters after sentence boundaries are capitalized. |
| 025 | Cleanup | Spoken symbols: at sign, slash, ampersand, parentheses. | Raw text, cleaned text. | Symbols are inserted and spacing is usable. |
| 026 | Cleanup | Cleanup disabled in Settings for a real recording. | Setting state, raw text, saved text. | Saved text remains close to raw transcript and skips cleanup transforms. |
| 027 | Injection | Auto-inject on with restore clipboard on. | Clipboard before/after, target output, injection result. | Target receives text and previous clipboard is restored. |
| 028 | Injection | Auto-inject on with restore clipboard off. | Clipboard before/after, target output. | Target receives text and clipboard remains transcript or expected app state. |
| 029 | Injection | Clipboard-only injection mode. | Setting state, target output, clipboard content. | No paste is sent; transcript is available on clipboard. |
| 030 | Injection | Manual paste after clipboard-only mode. | Clipboard content, manual paste target output. | Manual paste inserts exact cleaned transcript. |
| 031 | Injection | Paste test with focused Notepad. | Paste test notice and target output. | Test text appears in Notepad and notice is clear. |
| 032 | Injection | Paste test with no focused editable target. | Notice, runtime event, clipboard state. | App reports failure or limitation clearly without crashing. |
| 033 | Injection | Inject into browser contenteditable editor. | Browser URL, output text, injection result. | Text appears once at cursor. |
| 034 | Injection | Inject into IDE terminal or non-text area. | Target details, notice, clipboard state. | App handles unsupported target clearly and preserves data. |
| 035 | Injection | Escape key cancel during active recording. | Shortcut mode, notice, history state. | Recording is canceled and no transcript is injected or saved as complete. |
| 036 | Injection | Re-inject from History into a different app than original. | Session id, original app, new target app output. | History re-inject sends cleaned transcript to focused target. |
| 037 | Shortcuts | Ctrl+Win in push-to-talk mode. | Shortcut status, press/release result. | Press starts and release stops recording. |
| 038 | Shortcuts | Ctrl+Win+Space in push-to-talk mode. | Shortcut status, press/release result. | Press starts and release stops recording. |
| 039 | Shortcuts | Ctrl+Alt+Space in push-to-talk mode. | Shortcut status, press/release result. | Press starts and release stops recording. |
| 040 | Shortcuts | Ctrl+Shift+Space in push-to-talk mode. | Shortcut status, press/release result. | Press starts and release stops recording. |
| 041 | Shortcuts | Ctrl+Alt+D in push-to-talk mode. | Shortcut status, press/release result. | Press starts and release stops recording. |
| 042 | Shortcuts | Ctrl+Win in toggle mode. | Shortcut status, start/stop events. | First trigger starts, second trigger stops. |
| 043 | Shortcuts | Change hotkey at runtime and immediately use it. | Old/new hotkey, shortcut status event. | Old hotkey stops controlling app and new hotkey works. |
| 044 | Shortcuts | Pause shortcuts from Settings. | Shortcut status, attempted hotkey event. | Hotkey does not start recording while paused. |
| 045 | Shortcuts | Resume shortcuts from Settings. | Shortcut status, attempted hotkey event. | Hotkey works again after resume. |
| 046 | Shortcuts | Shortcut test detects active shortcut press. | Test state, notice, event. | Test reports detection and then exits test mode. |
| 047 | Shortcuts | Shortcut test timeout without pressing shortcut. | Test state, notice. | Timeout message is clear and app returns to idle test state. |
| 048 | Shortcuts | Conflict with existing app shortcut. | Target app, hotkey, shortcut status. | Atmospeak either registers reliably or reports conflict/unavailable clearly. |
| 049 | AI Polish | Concise style on a rambling transcript. | Original, polished text, provider latency. | Output is shorter, accurate, and saved where expected. |
| 050 | AI Polish | Formal style on casual transcript. | Original, polished text, style setting. | Tone becomes formal without changing meaning. |
| 051 | AI Polish | Casual style on formal transcript. | Original, polished text, style setting. | Tone becomes casual without adding unsupported claims. |
| 052 | AI Polish | Excited style on neutral announcement. | Original, polished text, style setting. | Output is energetic but still suitable and accurate. |
| 053 | AI Polish | Summarize style on 30+ second dictation. | Original length, summary, latency. | Summary captures key points and omits filler. |
| 054 | AI Polish | Polish same session multiple times. | Session id, each output, history state. | App remains stable and either updates predictably or reports no changes. |
| 055 | AI Polish | Polish a clean transcript with no meaningful changes. | Original, result metadata, notice. | Notice is concise and no harmful rewrite occurs. |
| 056 | AI Polish | Ollama-compatible provider success. | Endpoint, model, latency, output. | Local provider returns polished text and UI updates. |
| 057 | AI Polish | OpenAI-compatible provider success. | Endpoint, model, latency, output. | Remote-compatible provider works without exposing API key. |
| 058 | AI Polish | Polish disabled. | Setting state, attempted polish behavior. | App does not call a provider and communicates disabled state clearly. |
| 059 | AI Polish | Provider error recovery. | Error text, notice, session state. | Original transcript remains available and app recovers without crash. |
| 060 | Settings | Toggle restore clipboard on and off. | Setting value, save notice, injection behavior. | Saved setting persists and changes injection behavior. |
| 061 | Settings | Toggle auto-inject on and off. | Setting value, recording result. | Off mode saves transcript without injecting; on mode injects. |
| 062 | Settings | Toggle cleanup enabled on and off. | Setting value, raw/cleaned comparison. | Pipeline matches setting. |
| 063 | Settings | Toggle start with Windows on and off. | Setting value, save result/runtime event. | App reports success or OS rejection clearly. |
| 064 | Settings | Toggle auto-edit before paste on and off. | Setting value, recording output. | Auto polish runs only when enabled and provider is configured. |
| 065 | Settings | Toggle live preview enabled on and off. | Overlay behavior, transcript events. | Overlay preview follows setting. |
| 066 | Settings | Toggle final accuracy pass on and off. | Setting value, final transcript behavior. | Final pass follows setting without duplicate snippet expansion. |
| 067 | Settings | Switch recognition language to English. | Setting value, recording output. | Whisper command uses English and transcript remains accurate. |
| 068 | Settings | Switch recognition language to Auto-detect. | Setting value, model availability, output. | Auto mode works when multilingual model exists or reports model limitation. |
| 069 | Settings | Change floating bubble size small, medium, large. | Setting values, overlay screenshot. | Dock scale visibly changes without layout break. |
| 070 | Settings | Change floating bubble opacity from 20% to 100%. | Setting values, overlay screenshot. | Dock opacity updates and remains readable. |
| 071 | Settings | Change live preview interval through all options. | Setting value, preview cadence notes. | Preview remains stable and CPU/latency are acceptable. |
| 072 | Advanced | Inspect model inventory. | Inventory list, installed flags, active id. | Installed and unavailable models are shown accurately. |
| 073 | Advanced | Select bundled Base English model. | Model id, save result, status. | Bundled runtime remains ready and selected. |
| 074 | Advanced | Enable advanced runtime with valid whisper-cli and model paths. | Paths, runtime status, test recording. | Advanced source is ready and transcription works. |
| 075 | Advanced | Enable advanced runtime with invalid whisper-cli path. | Path, status, notice. | App reports incomplete runtime and blocks/recovers clearly. |
| 076 | Advanced | Enable advanced runtime with invalid model path. | Path, status, notice. | App reports incomplete runtime and does not lose settings. |
| 077 | Advanced | Switch bundled to advanced override and back. | Runtime status before/after, recording output. | Source switching is reliable and each save updates status. |
| 078 | Advanced | Custom instructions saved and used by polish. | Instructions, provider output. | Instructions persist and influence polish result when applicable. |
| 079 | History | Search by exact transcript word. | Search query, result count. | Matching rows appear and nonmatches disappear. |
| 080 | History | Search by app name. | Query, recent app data, result rows. | App-name search returns expected sessions. |
| 081 | History | Filter by date range. | Dates, expected sessions. | Only sessions in range are visible. |
| 082 | History | Filter by minimum word count. | Filter value, result rows. | Rows below threshold are hidden. |
| 083 | History | Filter by maximum word count. | Filter value, result rows. | Rows above threshold are hidden. |
| 084 | History | Export TXT. | File content, session id. | TXT contains cleaned transcript. |
| 085 | History | Export Markdown. | File content, session id. | Markdown contains expected transcript and metadata. |
| 086 | History | Export JSON. | JSON content, parse result. | JSON is valid and contains expected fields. |
| 087 | History | Export SRT. | SRT content, timestamp format. | SRT has valid cue format and transcript text. |
| 088 | History | Add and persist notes on a session. | Session id, note text before/after restart. | Notes save and reload. |
| 089 | Dictionary | Add new dictionary entry and use it in dictation. | Entry, raw/cleaned text. | Replacement applies. |
| 090 | Dictionary | Edit existing dictionary entry. | Old/new entry, cleaned text. | New replacement applies and old behavior stops. |
| 091 | Dictionary | Delete dictionary entry. | Entry id, cleaned text after deletion. | Deleted entry no longer applies. |
| 092 | Dictionary | Toggle dictionary entry disabled/enabled. | Entry state, cleaned text. | Replacement follows enabled state. |
| 093 | Dictionary | Duplicate phrase entries with different replacements. | Entries order, cleaned text. | Behavior is deterministic and not confusing. |
| 094 | Dictionary | Special characters in dictionary phrase/replacement. | Entry, cleaned text. | Regex escaping works and no crash occurs. |
| 095 | Snippets | Add snippet and use trigger in dictation. | Snippet, overlay preview, cleaned text. | Snippet expands once. |
| 096 | Snippets | Edit snippet body and trigger. | Old/new snippet, cleaned text. | Edited snippet applies and old trigger behavior is gone. |
| 097 | Snippets | Delete snippet. | Snippet id, cleaned text after deletion. | Deleted snippet no longer expands. |
| 098 | Snippets | Overlapping triggers, such as "ship" and "ship intro". | Snippets, cleaned text. | Expansion is deterministic and does not corrupt text. |
| 099 | Snippets | Multi-line and special-character snippet body. | Snippet body, cleaned text, target output. | Line breaks and special characters survive cleanup/injection. |
| 100 | System UX and recovery | Full system gauntlet: show/drag/reopen overlay, live transcript and FFT, full onboarding pass/fail paths, tray open/start/stop/quit, update no-update and available flows, privacy auto-delete during/after recording, no mic, too-short recording, whisper failure, empty transcript, unfocused target, shortcut registration failure, and database corruption recovery. | Split this gauntlet into timestamped sub-runs with screenshots/logs/runtime events for each failure or recovery path. | Each subsystem either succeeds or fails with clear recovery, no data loss, and a linked fix/retest for any unsatisfactory sub-run. |

## Execution Log Template

Append one entry per test run. If a fix is made, append a new retest entry rather than overwriting the failed run.

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

## Current Evidence Status

- Existing automated gates cover browser mock onboarding, hub, overlay, and selected component behavior, but they do not prove production recording quality.
- Existing manual tier scripts cover a subset of parity and regression checks, but not all 100 cases above.
- No committed audio fixture files were found under `src`, `src-tauri`, `tests`, or `docs` during this planning pass. The production run still needs fresh real recordings through `bun run tauri dev`.
