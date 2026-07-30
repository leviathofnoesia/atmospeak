# Polar product setup — Atmospeak Pro

Configure the existing **$69** one-time Atmospeak Pro product in the Nov Pax
Polar organization as follows.

## Benefits

1. **License Keys**
   - Prefix optional (e.g. `ATMO`)
   - **Do not** set a short absolute expiry that bricks the app — Pro uses
     online validate + a **3-year update window** enforced by the gated update
     Worker. Prefer no key expiry, or expiry far beyond the update window.
   - Enable activations if you want a device limit (recommended: 3).
   - Copy the benefit id into `ATMOSPEAK_POLAR_LICENSE_BENEFIT_ID`.

2. **File Downloads**
   - Upload the current `atmospeak-pro_<version>_x64-setup.exe` after each Pro
     release (or automate via Polar Files API from CI).
   - This is the purchase / reinstall path; in-app updates use the gated Worker.

## Checkout

- Create a Polar checkout link for the product.
- Set `ATMOSPEAK_POLAR_CHECKOUT_URL` / `VITE_ATMOSPEAK_POLAR_CHECKOUT_URL` to that URL
  (website Buy buttons and in-app upgrade link).

## App env (Pro builds)

```env
ATMOSPEAK_POLAR_ORGANIZATION_ID=...
ATMOSPEAK_POLAR_LICENSE_BENEFIT_ID=...
ATMOSPEAK_LICENSE_GRACE_DAYS=14
ATMOSPEAK_PRO_UPDATE_BASE=https://updates.novpax.org/atmospeak/pro
```

## Customer flow

1. Buy on Polar from novpax.org / checkout link.
2. Polar portal shows licence key + Pro installer download.
3. Install Pro → Hub **Pro** tab → paste key → Activate (online).
4. Offline grace (`ATMOSPEAK_LICENSE_GRACE_DAYS`) keeps Pro features working
   between online validates.
5. Pro update checks send licence headers to the Worker; expired update windows
   get no new builds (installed Pro keeps running).

## Server secrets (Worker / CI only)

Never ship these in the desktop app:

- `POLAR_ACCESS_TOKEN`
- R2 credentials / `ARTIFACT_SIGNING_SECRET`
