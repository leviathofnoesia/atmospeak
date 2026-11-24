# Streaming local ASR

Parakeet Unified English 0.6B Q8_0 runs locally through the pinned
transcribe.cpp CPU sidecar. The app sends 16 kHz mono PCM and Parakeet
consumes it in buffered chunks. Live dictation and WAV recovery share that
decoder. The build and license details are in [Model bootstrap](MODEL_BOOTSTRAP.md).

## Runtime and build

`scripts/build-asr-sidecars.ps1` stages the sidecar, portable CPU kernels,
and verified GGUF. Model preparation runs in a background worker. The model
stays resident for streaming sessions. Superseded startup generations terminate
their owned child before publication. Windows job ownership handles shutdown.

## Capture, recognition, and delivery

1. The CPAL callback downmixes input into a bounded, nonblocking queue. A
   recorder worker persists the recoverable WAV and incrementally resamples
   audio into 20 ms mono 16 kHz frames.
2. Audio and controls use separate bounded queues. Session IDs and sequences
   isolate sessions. Stop carries an accepted-audio barrier and final sequence
   so leading, middle, and tail losses can be counted.
3. The sidecar reader parses commands. One worker owns the native transcriber
   and stream. Dispatch wakes the parked worker directly, avoiding a fixed
   idle-polling delay on Stop.
   Preview freshness uses the frame's reader-to-worker queue age, rather than
   comparing wall time to an independent audio sample clock. Start delays and
   producer clock drift cannot permanently suppress previews. The native
   max_backlog_ms diagnostic reports that queue age.

4. Parakeet emits cumulative Partial text for live preview. Partials remain
   provisional. Dictionary/snippet context resets for every session; there is
   no fixed overlapping Whisper chunk commit path.
5. Stop drains accepted audio, stops the native stream, finalizes its tail,
   and emits metrics before Final. Delivery requires the matching authoritative
   result. A displayed partial does not authorize an early paste.
6. The app checkpoints the recording in History before final processing,
   analyzes bounded recent audio without changing the saved samples, applies
   cleanup and optional editing, and injects the chosen final text once.
   Audio-quality warnings are advisory, never authority to delete capture.
7. Cancel remains reachable during processing. Cancellation generations suppress
   stale results and delivery while retaining available audio/text for recovery.
   The following session starts with a fresh native stream.

## Faults and saved-WAV recovery

Material transport loss and native failures retain the durable recording.
Recovery uses an owned one-shot process of the same CPU engine:

```text
atmospeak-asr-cpu.exe --model <directory> --transcribe-wav <path>
```

The adapter concurrently drains bounded stdout/stderr, requires UTF-8 output,
and checks exit status. Cancellation or timeout kills, waits, and drains the
owned child. The deadline is 30 seconds plus the WAV-byte audio budget, capped
at 30 minutes. Failure retains the source and reports the error; it does not
switch engines or authorize pasting an unconfirmed partial.

## Protocol and verification

IPC is versioned, length-prefixed MessagePack over stdin/stdout, capped at
1 MiB per message. Streaming stdout is protocol-only; diagnostics use stderr.
Stale versions are rejected before model use. Protocol changes require a rebuilt
sidecar.

Recorded-audio checks cover known-phrase output, short/long recordings,
zero-drop healthy runs, stale protocols, missing-tail/middle-gap accounting,
cancellation without Final, the following session, and saved-WAV replay.
Recognition latency ends at Final; editing and paste are separate stages.
Historical ASR transcripts measure agreement, not human-ground-truth accuracy.

Use focused existing automated checks for changed behavior. Confirmed microphone
acceptance remains valid unless a concrete change or observed failure invalidates
it. Documentation and window-chrome updates require no repeat physical mic test.

## Final text

Deterministic cleanup runs only after the authoritative final transcript. It
preserves existing paragraph breaks, decimal numbers, times, URLs, negation,
and meaningful uses of "like". Fixed patterns compile once. Dictionary and
snippet replacements are literal (including dollar signs) and happen after
speech formatting; overlapping phrases prefer the longest match.

Use "bullet point apples next bullet pears" or "number one check the build
number two ship it" for explicit lists. An optional "end list" returns to prose.
Numbered commands must start at one and be sequential; ordinary first/second
phrases and ambiguous numbering remain prose. A list introduced within prose
needs a colon or an explicit "bullet list" / "numbered list" command.

"Scratch that" clears the current sentence or list item. "I mean" and numeric
or repeated-anchor corrections replace the identified tail. Uncertain uses of
"actually" remain literal. Restatement cleanup considers adjacent phrases only;
it cannot erase intervening sentences or paragraphs. Raw ASR remains in History.
Optional AI editing is separate and can still change wording.

Replacement matching keeps one earliest valid candidate per dictionary/snippet
pattern and advances only consumed or overlapping candidates. It preserves
Unicode case folding, word boundaries, longest overlaps, entry-order ties, and
literal replacement bodies without repeatedly scanning every unused term over
each remaining suffix.

Saved-recording desktop measurements pace frames against absolute monotonic
deadlines. Sleeping a fresh 20 ms after each frame accumulates scheduling drift,
inflates capture finalization, and is unsuitable for long-recording latency
comparisons.
