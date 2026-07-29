# Wispr Flow competitive audit

Checked against public product and technical material on 2026-07-27. This is a
product benchmark, not a claim about Wispr's private implementation.

## What Wispr publicly confirms

- Transcription runs in the cloud and requires an internet connection.
- Desktop dictation starts from a global hotkey; the documented Windows default
  is `Ctrl+Win`.
- Flow presents transcription in real time, applies AI formatting/commands, and
  learns vocabulary and correction preferences.
- Its stated engineering target is the complete ASR and LLM-formatted result
  within 700 ms after speech stops, with less than 200 ms each budgeted for ASR
  and LLM inference.
- Context can include nearby text, names on screen, and dedicated conversation
  context in Slack and Apple Messages.
- Wispr describes context-conditioned and personalized ASR plus personalized
  LLM formatting. It does not publicly identify one exact production model,
  checkpoint, vendor, quantization, or routing policy.

Sources:

- <https://docs.wisprflow.ai/articles/2772472373-what-is-flow>
- <https://docs.wisprflow.ai/articles/4678293671-feature-context-awareness>
- <https://wisprflow.ai/privacy>
- <https://wisprflow.ai/post/technical-challenges>

## What changed in Atmospeak

Atmospeak remains deliberately local and offline-first. The bundled
`base.en` model still guarantees first-run operation without a download.
Settings now also offers two current-generation, checksum-pinned upgrades:

- `large-v3-turbo-q5`: a multilingual, quantized Large v3 Turbo model from the
  official whisper.cpp model repository. The published GGML file is about
  547 MiB.
- `distil-large-v3.5`: the newest English Distil-Whisper release. Its model card
  reports stronger short-form robustness than the previous Distil Large v3 and
  a different accuracy/speed balance from Large v3 Turbo. The GGML file is about
  1.45 GiB.

The older `distil-large-v3` entry remains available so existing installations
and selected settings do not break.

Sources:

- <https://github.com/ggml-org/whisper.cpp/blob/master/models/README.md>
- <https://huggingface.co/distil-whisper/distil-large-v3.5>
- <https://huggingface.co/distil-whisper/distil-large-v3.5-ggml>

## Honest gap assessment

### P0 — make dictation boringly reliable

- Push-to-talk must always stop, transcribe, and paste once when either part of
  the held chord is released.
- Abandoned shortcut testing must never suppress the global dictation hook.
- Warm-host latency and injection success need native release telemetry and
  repeated real-microphone tests.

### P1 — reduce the post-stop wait

- Stream partial ASR results while recording, then reconcile them with the
  final transcript.
- Use VAD to bound trailing silence and begin final decoding immediately.
- Benchmark `base.en`, `small.en`, `large-v3-turbo-q5`, and
  `distil-large-v3.5` on the same short-dictation corpus and target hardware.
- Feed dictionary terms, snippet vocabulary, and safe nearby application text
  into recognition rather than applying all personalization after ASR.

### P2 — formatting and personalization

- Add an optional local rewrite layer with explicit, reversible commands.
  **Status (0.5.3):** deterministic Backtrack ships in cleanup (always with
  `cleanupEnabled`); default polish path is a bundled `llama-server` + curated
  GGUF (download on first enable). Ollama / OpenAI-compatible remain Advanced.
  History Undo/Redo AI edit is available.
- Learn corrections with per-app scope and a visible way to inspect or delete
  learned preferences.
- Introduce app-aware formatting only after the privacy boundary and capture
  permissions are clear.

### P3 — language and platform breadth

- Add multilingual model selection, language detection, and code-switching
  tests.
- Treat macOS, mobile, and sync as separate product programs rather than
  weakening the Windows reliability gate.

The near-term differentiator is not pretending a local Whisper checkpoint is
Wispr's private stack. It is delivering a trustworthy offline path, transparent
models, no account requirement, and predictable user control while closing the
latency and context gaps with measured work.
