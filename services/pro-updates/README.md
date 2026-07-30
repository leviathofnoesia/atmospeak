# Atmospeak Pro gated updates

Cloudflare Worker that serves Pro `latest.json` and installer artifacts only to
clients that present a valid Polar licence + activation (and are inside the
3-year update window).

## Bring-up

1. Create R2 bucket `atmospeak-pro`.
2. `cd services/pro-updates && bun install`
3. Set secrets:
   ```bash
   bunx wrangler secret put POLAR_ACCESS_TOKEN
   bunx wrangler secret put POLAR_ORGANIZATION_ID
   bunx wrangler secret put POLAR_LICENSE_BENEFIT_ID   # optional
   bunx wrangler secret put ARTIFACT_SIGNING_SECRET   # required for signed artifact URLs
   ```
4. Upload release artifacts:
   - `atmospeak/pro/latest.json`
   - `atmospeak/pro/artifacts/atmospeak-pro_<ver>_x64-setup.exe`
5. Point DNS `updates.novpax.org` at the Worker (or use workers.dev while testing).
6. Deploy: `bun run deploy`

## Client headers

| Header | Value |
| --- | --- |
| `X-Atmospeak-License` | Polar licence key |
| `X-Atmospeak-Activation` | Activation id from Polar activate |

The Pro desktop build sends these via `tauri_plugin_updater` `UpdaterBuilder::header`
from `check_pro_update`.

## Free channel

Free builds do **not** use this Worker. They poll

`https://www.novpax.org/downloads/atmospeak/free/latest.json`

on the Nov Pax marketing host (Vercel `public/downloads/`).
