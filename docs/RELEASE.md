# Atmospeak Release Process

Atmospeak ships **two channels**. Free and Pro are separate binaries and policies.

## Free channel (public)

Artifacts under `release/free/`:

- NSIS installer (primary download)
- MSI installer
- Portable zip
- NSIS updater signature + `latest.json`
- `SHA256SUMS.txt`

**Advertised CDN**

```text
https://downloads.novpax.org/atmospeak/free/
https://downloads.novpax.org/atmospeak/free/latest.json
```

Upload every file from `release/free/` to that CDN prefix after each free
release. GitHub Releases may mirror the same files but is not the user-facing URL.

App updater endpoint (free builds): the CDN `latest.json` above
(`src-tauri/tauri.conf.json`).

## Pro channel (gated)

Artifacts under `release/pro/`:

- `atmospeak-pro_<version>_x64-setup.exe` (+ sig)
- MSI / portable as produced
- `latest.json` (manifest body for private R2; Worker rewrites artifact URLs)

Upload to private R2 (`atmospeak/pro/…`), refresh the Polar File Download
benefit, and ensure the Pro update Worker can read the manifest. See
[`PRO_BUILD.md`](PRO_BUILD.md), [`POLAR.md`](POLAR.md), and
[`../services/pro-updates/README.md`](../services/pro-updates/README.md).

## Local Release Build

```powershell
$env:ATMOSPEAK_FREE_CDN_BASE = "https://downloads.novpax.org/atmospeak/free"
$env:TAURI_SIGNING_PRIVATE_KEY_PATH = "$env:USERPROFILE\.tauri\atmospeak\updater.key"
bun run release:build          # free → release/free/
bun run release:build:pro      # pro  → release/pro/
```

The private updater key must stay outside the repository. The matching public
key is committed in `src-tauri/tauri.conf.json`.

## Authenticode (Nov Pax)

Windows SmartScreen requires Authenticode / Azure Trusted Signing under the
**Nov Pax** publisher identity before charging for Pro. Updater minisign
signatures are separate from Authenticode.

## Install/Uninstall Smoke

```powershell
bun run release:test-install
```

## Storefront

Canonical download + buy: **https://www.novpax.org/projects/atmospeak**  
This repo’s `website/` GitHub Pages deploy is a transitional mirror and should
redirect or defer to novpax.org once that page is live.
