# Phase B — Persistent ASR host (shipped in 0.3.0)

Phase A ran `whisper-cli.exe` once per utterance, reloading the model every time.
Phase B keeps a `whisper-server.exe` resident so the model stays warm.

**Chosen option: B1** — bundle the server from the same upstream release archive the
CLI already comes from. B2 (`whisper-rs` in-process) and B3 (custom host) were not
needed: `whisper-server.exe` ships in `whisper-bin-x64.zip` alongside `whisper-cli.exe`,
so this is a bundling change rather than a source build.

## How it works

| Piece | Location |
|-------|----------|
| Process supervision, readiness, HTTP | `src-tauri/src/services/asr_host.rs` |
| Backend selection + CLI fallback | `src-tauri/src/services/transcriber.rs` |
| Startup warmup (background thread) | `start_asr_host` in `src-tauri/src/lib.rs` |
| Binary resolution | `runtime::resolve_server` in `src-tauri/src/services/runtime.rs` |

- The server binds an **ephemeral loopback port** (never a fixed one) and is reached at
  `POST http://127.0.0.1:<port>/inference` with a multipart `file` field and
  `response_format=text`.
- Startup happens on a **background thread**, so loading the model never delays the
  window. Until the host is warm, dictation transparently uses the CLI.
- Both whisper subprocesses spawn with `CREATE_NO_WINDOW` (`services/proc.rs`) so no
  console flashes on each utterance.

## Failure behaviour

The host is always optional. Dictation must never fail because of it.

| Situation | Result |
|---|---|
| `whisper-server.exe` missing | CLI backend; `asr-host-unavailable` runtime event |
| Server fails to start or never becomes ready | CLI backend; `asr-host-error` event |
| Server dies between utterances | Respawned on the next utterance |
| Request fails mid-session | That utterance falls back to the CLI (`asr-host-fallback`), server is torn down and respawned next time |
| Transcript is empty | Treated as **no speech**, not a host failure — no pointless CLI retry |

## Lifetime

The child is assigned to a Windows **job object** with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`,
so it dies with Atmospeak even on a crash or force-kill. `RunEvent::Exit` also shuts it
down on the clean path. Verified: hard-killing `atmospeak.exe` leaves no orphan.

## Kill-switch

```powershell
$env:ATMOSPEAK_WHISPER_HOST = "0"   # force the one-shot CLI backend
```

## Metrics

`StageMetrics.asr_backend` is `"host"` or `"cli"` per utterance, so the two paths are
directly comparable in `tests/manual/production-run-log.md`.

## Measured latency (base.en, CPU, 6.42 s of synthesized speech)

| Backend | ASR time |
|---|---|
| CLI (cold model each run) | 1.98 – 2.62 s |
| Host (warm) | 1.55 – 1.93 s |

Model load costs roughly **0.7 s per utterance**, and that is what the host removes.
Because `base.en` is small and loads fast, the saving is a fixed offset rather than a
multiple — the win is proportionally larger on short utterances (where load dominates
compute) and on larger models. The remaining time is actual inference; reaching the
spec's ≤700 ms p50 total for typical short phrases depends on utterance length.
Real numbers belong in the run log from mic dogfooding, not from synthesized audio.
