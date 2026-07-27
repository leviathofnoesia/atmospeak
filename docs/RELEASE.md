# Atmospeak Release Process

Atmospeak ships Windows-first release artifacts:

- NSIS installer: primary user download.
- MSI installer: enterprise-friendly fallback.
- Portable zip: unzip-and-run build containing `Atmospeak.exe`, sidecar runtime, and bundled resources.
- NSIS updater signature: signed Tauri v2 updater artifact used by `latest.json`.
- `latest.json`: Tauri updater metadata for GitHub Releases.
- `SHA256SUMS.txt`: checksum manifest for public verification.

## Local Release Build

```powershell
$env:ATMOSPEAK_RELEASE_REPO = "leviathofnoesia/atmospeak"
$env:TAURI_SIGNING_PRIVATE_KEY_PATH = "$env:USERPROFILE\.tauri\atmospeak\updater.key"
bun run release:build
```

The private updater key must stay outside the repository. The matching public
key is committed in `src-tauri/tauri.conf.json`.

The app uses Tauri's `createUpdaterArtifacts: true` mode so Windows builds emit
updater signatures for the NSIS and MSI installers. The public installer
download and the updater artifact both use the NSIS `.exe`.

## GitHub Release

Upload every file from `release/` to the `leviathofnoesia/atmospeak` release
tag matching the application version. The updater endpoint is:

```text
https://github.com/leviathofnoesia/atmospeak/releases/latest/download/latest.json
```

### Repository-rename updater bridge

Atmospeak builds released before the repository rename poll the legacy feed:

```text
https://github.com/leviathofnoesia/wind-speak/releases/latest/download/latest.json
```

Version 0.3.1 is the signed bridge release. Before publishing it:

1. Publish the signed installer, installer signature, checksums, and
   `latest.json` in the renamed `leviathofnoesia/atmospeak` repository.
2. Confirm both the legacy URL above and the new Atmospeak URL return the same
   `latest.json` through GitHub's repository-rename redirect.
3. Confirm both feeds resolve the installer named by `latest.json` and that its
   signature matches the embedded Tauri updater public key.
4. Install an older build that uses the legacy endpoint and verify it discovers
   and installs 0.3.1.

The 0.3.1 binary uses the new Atmospeak endpoint. Do not retire or break
GitHub's legacy repository redirect until all supported pre-0.3.1 installations
have been upgraded or otherwise sunset.

## Unsigned Windows Prototype

This milestone does not include Authenticode code signing. Windows SmartScreen
may warn until a trusted certificate or Azure Trusted Signing profile is wired
into Tauri's Windows signing config.

Tauri updater signatures are separate from Windows code signing. The updater
verifies that update artifacts match the public key embedded in the app. The
signature stored in `latest.json` is the content of
`atmospeak_<version>_x64-setup.exe.sig`.

## Install/Uninstall Smoke

```powershell
bun run release:test-install
```

The script installs the NSIS build into a temp directory, checks the executable
and bundled runtime/model resources, launches briefly, uninstalls silently, and
verifies the executable is removed.
