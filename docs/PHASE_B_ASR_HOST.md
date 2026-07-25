# Phase B — Persistent ASR host (scaffold)

Phase A ships with **one-shot** `whisper-cli.exe` per utterance. Stock bundled CLI has **no** server/keep-alive/stdin protocol. Do not invent one.

## Why host?

Cold process + model load dominates latency (often multi-second). A resident model is required for “snappy” release→paste targets (p50 ≤ 1s).

## Options

| Option | Artifact | Notes |
|--------|----------|-------|
| **B1** | Bundle `whisper-server` (or equivalent) from whisper.cpp | New binary + resources + license + build script under `scripts/` |
| **B2** | In-process `whisper-rs` / ggml bindings | MSVC/link cost; no sidecar process |
| **B3** | Custom Atmospeak host linking whisper | Same complexity class as B1/B2 |

## Metrics

Phase A already labels `asr_backend: "cli"` on `StageMetrics`. A future host should set `asr_backend: "host"` or `"inprocess"`.

## Kill-switch (reserved)

`ATMOSPEAK_WHISPER_HOST=0` is reserved to force CLI fallback once a host exists.

## Non-goals for this scaffold

- No production host process in Phase A
- No fake JSON-lines protocol around stock `whisper-cli.exe`
