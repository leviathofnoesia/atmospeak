# Advanced Runtime Overrides

Atmospeak bundles the Windows x64 CPU `whisper.cpp` runtime and
`ggml-base.en.bin` model. Fresh installs should not ask users to download a
model or paste runtime paths.

Use this document only when replacing the bundled engine for development,
benchmarking, or larger local model tests.

## Bundled Runtime Layout

```text
src-tauri/binaries/whisper-cli-x86_64-pc-windows-msvc.exe
src-tauri/resources/whisper-runtime/whisper-cli.exe
src-tauri/resources/whisper-runtime/*.dll
src-tauri/resources/models/ggml-base.en.bin
```

The Tauri bundle also includes `resources/ACKNOWLEDGEMENTS.md`.

## Override Paths

1. Open **Advanced**.
2. Enable **Use advanced runtime override**.
3. Set `whisper-cli.exe`.
4. Set a compatible GGML model path.
5. Save runtime settings.

Atmospeak returns to bundled defaults as soon as the override toggle is off.

## Refreshing Bundled Assets

Official whisper.cpp Windows releases are published at:

```text
https://github.com/ggml-org/whisper.cpp/releases
```

The default model source is:

```text
https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
```

After replacing runtime files, run the full build and Tauri package checks.
