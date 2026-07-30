# Atmospeak Pro build & dual channel

Atmospeak ships two Windows binaries:

| Channel | Feature | Updater | Licence |
| --- | --- | --- | --- |
| Free | default (no `pro`) | `https://downloads.novpax.org/atmospeak/free/latest.json` | none |
| Pro | `--features pro` + `tauri.pro.conf.json` | `https://updates.novpax.org/atmospeak/pro/latest.json` (auth) | Polar online |

## Build

```powershell
# Free
bun run release:build

# Pro
bun run release:build:pro
```

Outputs land in `release/free/` or `release/pro/`.

## Authenticode

Sign **both** channels with the Nov Pax publisher identity before selling Pro.
Tauri updater signatures (`TAURI_SIGNING_*`) are separate from Authenticode.

## Private Pro modules

`src-pro/` is linked only when `pro` is enabled. For production hardening, move
it to a private repository and point Pro CI at that source so Pro feature code
is absent from the public free remote. See [`src-pro/README.md`](../src-pro/README.md).

## Gated Pro updates

See [`services/pro-updates/README.md`](../services/pro-updates/README.md).
