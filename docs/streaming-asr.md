# Streaming local ASR

Atmospeak 0.5 keeps microphone audio and transcription on the local machine.
From 0.5.1 the sidecar splits ingestion from inference so capture never stalls
behind a decode. From 0.5.2, commits are more aggressive and short warm clips
can finish on the resident host when streaming has not already committed
mid-utterance. From 0.5.3, **all ≤5 s** utterances prefer the warm batch host
(even if a dock partial appeared), deterministic Backtrack runs in cleanup, and
optional LLM polish can rewrite before paste. The recorder sends bounded 20 ms,
mono 16 kHz PCM frames to a crash-isolated `atmospeak-asr-host` process while
retaining the recording for history and batch fallback.

## Runtime selection

The release build produces CPU and Vulkan hosts from the same locked Rust and
whisper.cpp dependency graph. Startup tries Vulkan, then CPU, then the existing
warm `whisper-server`, and finally `whisper-cli`. Set
`ATMOSPEAK_STREAMING_ASR=0`, `ATMOSPEAK_ASR_BACKEND=cpu`, or
`ATMOSPEAK_WHISPER_HOST=0` to exercise rollback paths.

`scripts/build-asr-sidecars.ps1` verifies the pinned Silero VAD model checksum
and creates both sidecar executables. It requires CMake, libclang, and (for the
Vulkan variant) the Vulkan SDK.

## Session flow

1. The CPAL callback downmixes a device frame and uses a bounded, non-blocking
   queue.
2. A recorder worker appends the lossless capture, incrementally resamples it,
   and queues 20 ms PCM frames for the sidecar writer. Every hop feeds the same
   dropped-frame counter.
3. Inside `atmospeak-asr-host`, a reader thread only parses stdin. An inference
   worker owns the session: Silero VAD runs on the trailing three seconds of the
   uncommitted chunk (every ~100 ms of fresh audio), commits after ~300 ms of
   silence or a ~2 second forced split, and runs rolling six-second previews at
   most once per second — skipping previews while decode backlog is high so
   `StopSession` is not starved.
4. Stable chunks are reconciled with their overlap. Dictionary and snippet
   context is prompt-only during streaming. Capture always uses the streaming
   sidecar when it is available; the live-preview setting no longer disables
   that hot path.
5. Stop detaches capture before acknowledging the shortcut, flushes the writer,
   and sends `StopSession` immediately so the host finalizes the uncommitted
   tail while WAV teardown runs in parallel. Stop cancels pending preview work
   and always decodes the uncommitted tail. For short warm clips (≤ ~5 s) with
   no streaming partials, Atmospeak cancels streaming finalize and uses the
   warm resident host instead. Cleanup/snippet expansion and the single paste
   run once after the final; clipboard restore is off the inject critical path.
6. Material streaming loss (≥ ~250 ms of dropped frames) or a failed stop leaves
   the full local recording available to the legacy batch path, which prefers
   the lazy resident `whisper-server` over a cold one-shot `whisper-cli`. An
   incomplete streaming hypothesis is never pasted.

IPC is versioned, length-prefixed MessagePack over stdin/stdout. Protocol
frames are capped at 1 MiB; stdout is protocol-only and logs go to stderr.
The protocol types live in `src-asr-protocol`. The reader/worker split is
internal to the sidecar and does not bump the protocol version.
