# Wind Speak Release Process

Wind Speak ships Windows-first release artifacts:

- NSIS installer: primary user download.
- MSI installer: enterprise-friendly fallback.
- Portable zip: unzip-and-run build containing `wind-speak.exe`, sidecar runtime, and bundled resources.
- NSIS updater signature: signed Tauri v2 updater artifact used by `latest.json`.
- `latest.json`: Tauri updater metadata for GitHub Releases.
- `SHA256SUMS.txt`: checksum manifest for public verification.

## Local Release Build

```powershell
$env:WIND_SPEAK_RELEASE_REPO = "leviathofnoesia/wind-speak"
$env:TAURI_SIGNING_PRIVATE_KEY_PATH = "$env:USERPROFILE\.tauri\wind-speak\updater.key"
bun run release:build
```

The private updater key must stay outside the repository. The matching public
key is committed in `src-tauri/tauri.conf.json`.

The app uses Tauri's `createUpdaterArtifacts: true` mode so Windows builds emit
updater signatures for the NSIS and MSI installers. The public installer
download and the updater artifact both use the NSIS `.exe`.

## GitHub Release

Create or reuse `leviathofnoesia/wind-speak`, then upload every file from
`release/` to a release tag such as `v0.1.7`. The updater endpoint is:

```text
https://github.com/leviathofnoesia/wind-speak/releases/latest/download/latest.json
```

## Unsigned Windows Prototype

This milestone does not include Authenticode code signing. Windows SmartScreen
may warn until a trusted certificate or Azure Trusted Signing profile is wired
into Tauri's Windows signing config.

Tauri updater signatures are separate from Windows code signing. The updater
verifies that update artifacts match the public key embedded in the app. The
signature stored in `latest.json` is the content of
`Wind-Speak_<version>_x64-setup.exe.sig`.

## Install/Uninstall Smoke

```powershell
bun run release:test-install
```

The script installs the NSIS build into a temp directory, checks the executable
and bundled runtime/model resources, launches briefly, uninstalls silently, and
verifies the executable is removed.
