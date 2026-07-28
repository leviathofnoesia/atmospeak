# Streaming local ASR

Atmospeak 0.5 keeps microphone audio and transcription on the local machine.
The recorder sends bounded 20 ms, mono 16 kHz PCM frames to a crash-isolated
`atmospeak-asr-host` process while retaining the recording for history and
batch fallback.

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
   and queues 20 ms PCM frames for the sidecar writer.
3. Silero VAD commits segments after 500 ms of silence or a 15 second forced
   split. Rolling six-second previews run no more than once per second and
   throttle or suspend under backlog.
4. Stable chunks are reconciled with their 500 ms overlap. Dictionary and
   snippet context is prompt-only during streaming.
5. Stop detaches capture before acknowledging the shortcut, flushes only the
   uncommitted tail, runs cleanup/snippet expansion once, and pastes once.
6. Any streaming failure leaves the full local recording available to the
   legacy batch path; an incomplete streaming hypothesis is never pasted.

IPC is versioned, length-prefixed MessagePack over stdin/stdout. Protocol
frames are capped at 1 MiB; stdout is protocol-only and logs go to stderr.
The protocol types live in `src-asr-protocol`.
