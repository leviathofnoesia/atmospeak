# Atmospeak Release Process

Atmospeak ships **two channels**. Free and Pro are separate binaries and policies.

## Free channel (public)

Artifacts under `release/free/`:

- NSIS installer (primary download)
- MSI installer
- Portable zip
- NSIS updater signature + `latest.json`
- `SHA256SUMS.txt`

**Advertised download base** (Nov Pax marketing site, Vercel `public/downloads/`):

```text
https://www.novpax.org/downloads/atmospeak/free/
https://www.novpax.org/downloads/atmospeak/free/latest.json
```

Copy every file from `release/free/` into the novpax.org repo at
`public/downloads/atmospeak/free/` and deploy marketing. There is **no**
`downloads.novpax.org` host.

GitHub Releases may mirror the same files but is not the user-facing URL.

App updater endpoint (free builds): the www path above
(`src-tauri/tauri.conf.json`).

## Pro channel (gated)

Artifacts under `release/pro/`:

- `atmospeak-pro_<version>_x64-setup.exe` (+ sig)
- MSI / portable as produced
- `latest.json` (for gated Worker when deployed)

**First purchase / reinstall:** Polar File Downloads benefit (configure when the
first Pro EXE exists).

**In-app Pro updates:** `updates.novpax.org` Worker (`services/pro-updates/`) —
Atmospeak/Cloudflare follow-up; not blocking Polar checkout or File Downloads.

See [`PRO_BUILD.md`](PRO_BUILD.md), [`POLAR.md`](POLAR.md), and
[`../services/pro-updates/README.md`](../services/pro-updates/README.md).

## Local Release Build

```powershell
$env:ATMOSPEAK_FREE_CDN_BASE = "https://www.novpax.org/downloads/atmospeak/free"
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
