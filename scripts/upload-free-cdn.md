# Upload free release artifacts to novpax.org

Free installers are hosted **same-origin** on the Nov Pax marketing site
(Vercel `public/downloads/atmospeak/free/`), matching Sanctuary Desktop.

Canonical base:

```text
https://www.novpax.org/downloads/atmospeak/free/
```

Do **not** use `downloads.novpax.org` (no CNAME / separate downloads host).

## Publish

1. Build free channel: `bun run release:build` → `release/free/`
2. In the **novpax.org** repo, copy into `public/downloads/atmospeak/free/`:
   - `latest.json`
   - `atmospeak_<ver>_x64-setup.exe` (+ `.sig` if present)
   - MSI / portable / `SHA256SUMS.txt` as desired
3. Deploy the marketing site.

Pro purchase/reinstall uses Polar File Downloads. The gated Pro update Worker
(`services/pro-updates/`, `updates.novpax.org`) is a separate Atmospeak/Cloudflare
follow-up — not required for first Pro sales.
