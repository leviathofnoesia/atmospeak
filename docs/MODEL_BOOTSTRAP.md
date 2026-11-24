# Bundled English speech model

Build in an E: checkout with Rust, CMake, and Visual Studio Build Tools
with Desktop development with C++.

```powershell
powershell -ExecutionPolicy Bypass -File scripts/build-asr-sidecars.ps1
```

The build uses transcribe.cpp at revision
`2778ae4bf5ca5f394d129a39467f4ad9552dc34f` and stages its runtime-selected
CPU kernels beside the speech sidecar. The weights are
`parakeet-unified-en-0.6b-Q8_0.gguf` from the Handy conversion at revision
`7e948f21b7bdbac698d3318db9d350f1096f3b6c`. The build and the speech
sidecar verify the 731,357,568-byte file against SHA-256
`4b50b6dd862bf6e346929aaf4f5eaacec003bfa3f56462d6c874b41ef2f38795`.

Set `ATMOSPEAK_ASR_SELECTION_ROOT` to an E: folder with the verified GGUF to
reuse a cached download. The app bundles the weights and runs offline after
installation. Attribution and the NVIDIA and MIT licenses ship in
`resources/transcription-licenses`.

Both live dictation and saved-WAV replay use Parakeet's buffered stream.
The app and sidecar enforce the same protocol version.
