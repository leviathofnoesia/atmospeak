# Streaming local ASR

Atmospeak 0.5 keeps microphone audio and transcription on the local machine.
From 0.5.1 the sidecar splits ingestion from inference so capture never stalls
behind a decode. From 0.5.2, commits are more aggressive and short warm clips
can finish on the resident host when streaming has not already committed
mid-utterance. From 0.5.3, short clips briefly preferred the warm batch host
even when a dock partial appeared. From **1.0.0**, short clips stay on
**streaming finalize** (Vulkan first). Release→paste now prefers the cleaned
live hypothesis built during the hold, so paste no longer waits on Final when
the orb already shows text; finalize remains the fallback when no preview is
ready. The recorder sends bounded 20 ms, mono 16 kHz PCM frames to a
crash-isolated `atmospeak-asr-host` process while retaining the recording for
history and batch fallback.

## Runtime selection

The release build produces CPU and Vulkan hosts from the same locked Rust and
whisper.cpp dependency graph. Startup tries Vulkan, then CPU, then the existing
warm `whisper-server`, and finally `whisper-cli`. Set
`ATMOSPEAK_STREAMING_ASR=0`, `ATMOSPEAK_ASR_BACKEND=cpu`, or
`ATMOSPEAK_WHISPER_HOST=0` to exercise rollback paths.

`scripts/build-asr-sidecars.ps1` verifies the pinned Silero VAD model checksum
and creates both sidecar executables. It requires CMake, libclang, and (for the
Vulkan variant) the Vulkan SDK. On Windows it prefers a short target dir
(`C:\asrb` or `ATMOSPEAK_ASR_TARGET_DIR`) and Ninja when available — long-path
MSVC builds produced a slow ~3.3 MB CPU sidecar that could not keep up realtime.

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
   `StopSession` is not starved. Decode reuses one `WhisperState`, uses
   single-segment greedy params, and honors an abort flag so StopSession is not
   stuck behind an in-flight force-split.
4. Stable chunks are reconciled with their overlap. Lone silence markers
   (`[BLANK_AUDIO]`, `[silence]`, …) are dropped; real filler words are kept.
   Dictionary and snippet context is prompt-only inside the sidecar. Atmospeak
   also runs cleanup/snippet expansion on each live `Partial` / `StableSegment`
   into a paste-ready buffer so the orb shows the same text release will paste.
   Capture always uses the streaming sidecar when it is available; the
   live-preview setting no longer disables that hot path.
5. On release, if the paste-ready buffer is non-empty, Atmospeak pastes that
   cleaned hypothesis immediately after mic stop (no wait on Final, quality
   gate, or WAV). The orb advances to Pasted; host cancel, WAV teardown, and
   session persistence run after paste. Clipboard restore stays off the inject
   critical path. When no usable preview exists yet, stop falls back to the
   prior path: finalize capture, quality gate, `StopSession` / `await_final`
   (or batch), cleanup, then paste.
6. Material streaming loss (≥ ~250 ms of dropped frames) or a failed stop on
   the slow path leaves the full local recording available to the legacy batch
   path, which prefers the lazy resident `whisper-server` over a cold one-shot
   `whisper-cli`.

IPC is versioned, length-prefixed MessagePack over stdin/stdout. Protocol
frames are capped at 1 MiB; stdout is protocol-only and logs go to stderr.
The protocol types live in `src-asr-protocol`. The reader/worker split is
internal to the sidecar and does not bump the protocol version.
