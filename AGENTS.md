# Atmospeak Free

- The Windows app and CPU speech sidecar share the protocol in `src-asr-protocol`. Rebuild both when the wire format changes.
- The Parakeet model hash and size must agree in the speech sidecar, bootstrap, and WAV verification. The Tauri resources list also drives portable packaging. Runtime inventory and batch replay validate the same model file through `is_model_file_ready`.
- Dictation must checkpoint capture to local History before paste. A missing or incomplete streaming final is retried from that same WAV. Later saves update the existing checkpoint so deleting it during processing stays effective.
