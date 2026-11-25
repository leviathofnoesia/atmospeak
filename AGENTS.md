# Atmospeak Free

- The Windows app and CPU speech sidecar share the protocol in `src-asr-protocol`. Rebuild both when the wire format changes.
- Regenerate the shared GGUF catalog with `bun run speech:sync-catalog` after changing the pinned Handy package or engine. Preserve the Parakeet and Nemotron IDs and files used by existing profiles. All speech weights live in the per-user `models` directory, outside the installers.
- The catalog marks native preview support. The sidecar checks the loaded model before opening a stream. Batch models and streaming recovery both read the same durable WAV, with the selected dictation language.
- The Parakeet verification fixture used by the native smoke scripts must match the shared Parakeet hash and size.
- Dictation must checkpoint capture to local History before paste. A missing or incomplete streaming final is retried from that same WAV. Later saves update the existing checkpoint so deleting it during processing stays effective.
