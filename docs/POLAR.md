# Polar product setup — Atmospeak Pro

Production product (Nov Pax Polar org):

| Field | Value |
| --- | --- |
| Product | Atmospeak Pro License |
| product_id | `184d5727-0fbe-475c-b6c0-32fd93461cf9` |
| organization_id | `97f0d813-d25f-4cc4-b934-fd4705a01c47` |
| Price | $69.00 USD one-time |
| License benefit | `b4e88474-01fa-450c-9aac-07bd92d8e887` (prefix `ATMO`, 3 activations, no key expiry) |
| Checkout | https://buy.polar.sh/polar_cl_a4ccEACEPXCGN3mZk0JDyDVXzeob3K8CSpYeB34qQsE |

## Benefits

1. **License Keys** — configured as above. Prefer **no short key expiry**; the
   3-year update window is enforced by the gated update Worker (when deployed),
   not by killing the licence key.
2. **File Downloads** — attach ≥1 Pro installer when the first
   `atmospeak-pro_*_x64-setup.exe` exists (purchase / reinstall path). In-app
   gated updates are a separate follow-up.

## App env (Pro builds)

Organization and benefit IDs are **compile-time constants** in release Pro
builds. `ATMOSPEAK_POLAR_ORGANIZATION_ID` / `ATMOSPEAK_POLAR_LICENSE_BENEFIT_ID`
only override those anchors in **debug** builds (local Polar sandboxes).

```env
# Debug-only overrides (ignored in release):
# ATMOSPEAK_POLAR_ORGANIZATION_ID=97f0d813-d25f-4cc4-b934-fd4705a01c47
# ATMOSPEAK_POLAR_LICENSE_BENEFIT_ID=b4e88474-01fa-450c-9aac-07bd92d8e887
ATMOSPEAK_POLAR_CHECKOUT_URL=https://buy.polar.sh/polar_cl_a4ccEACEPXCGN3mZk0JDyDVXzeob3K8CSpYeB34qQsE
ATMOSPEAK_LICENSE_GRACE_DAYS=14
ATMOSPEAK_PRO_UPDATE_BASE=https://updates.novpax.org/atmospeak/pro
```

## Customer flow

1. Buy on Polar from novpax.org / checkout link.
2. Polar portal shows licence key + Pro installer download (once File Download is attached).
3. Install Pro → Hub **Pro** tab → paste key → Activate (online).
4. Offline grace keeps Pro features working between online validates.
5. When the gated Worker is live, Pro update checks send licence headers; expired
   update windows get no new builds (installed Pro keeps running).

## Server secrets (Worker / CI only)

Never ship these in the desktop app or commit them:

- `POLAR_ACCESS_TOKEN`
- R2 credentials / `ARTIFACT_SIGNING_SECRET`
