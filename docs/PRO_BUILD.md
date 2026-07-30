# Atmospeak Pro build (private)

This **public** repository is **free-only**.

Atmospeak Pro sources, Polar licence adapters, Pro UI, gated-update Worker, and
Pro packaging live in the private Nov Pax monorepo:

[`leviathofnoesia/Nov-Pax-Web`](https://github.com/leviathofnoesia/Nov-Pax-Web)
→ `products/atmospeak-pro/`

## Channels

| Channel | Where | Updater | Licence |
| --- | --- | --- | --- |
| Free | this repo | `https://www.novpax.org/downloads/atmospeak/free/latest.json` | none |
| Pro | Nov-Pax-Web `products/atmospeak-pro` | `https://updates.novpax.org/atmospeak/pro/latest.json` (auth; Worker follow-up) | Polar online |

## Free build (this repo)

```powershell
bun run release:build
```

Outputs: `release/free/`. Publish per [`scripts/upload-free-cdn.md`](../scripts/upload-free-cdn.md).

## Pro build (private)

```powershell
cd products/atmospeak-pro   # in Nov-Pax-Web
powershell -ExecutionPolicy Bypass -File scripts/package-pro.ps1
```

See that product README for assemble/dev details. Do **not** add a private git
dependency to this public `Cargo.toml`.
