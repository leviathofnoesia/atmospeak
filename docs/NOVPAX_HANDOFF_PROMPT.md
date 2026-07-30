# Handoff prompt — paste into the novpax.org agent

Copy everything below the line into a new chat in the **novpax.org** project.

---

## Mission

Finish Nov Pax storefront + hosting integration for **Atmospeak** (desktop dictation). The Atmospeak app repo already implemented Free vs Pro separation in code. Your job is everything that lives on **novpax.org**, **Polar (Nov Pax org)**, and **Cloudflare DNS/R2/Workers** for downloads and gated Pro updates.

Do as much as you can in this repo / your infra. Do not rewrite Atmospeak app code unless you discover a blocker that requires a small PR back — prefer documenting blockers for the Atmospeak repo.

Atmospeak repo path on the author’s machine (read-only reference if available): `C:\Users\billy\Documents\atmospeak`  
GitHub: `https://github.com/leviathofnoesia/atmospeak`

Key docs already written there:

- `docs/NOVPAX_ATMOSPEAK_PAGE.md` — product page copy
- `docs/POLAR.md` — Polar product / benefits / customer flow
- `docs/PRO_BUILD.md` — free vs Pro build channels
- `docs/RELEASE.md` — CDN upload expectations
- `services/pro-updates/` — Cloudflare Worker source for gated Pro updates
- `scripts/upload-free-cdn.md` — free CDN upload example
- `.env.example` — env var names for Pro builds + CDN

---

## Locked product decisions (do not reopen)

| Decision | Choice |
| --- | --- |
| Storefront | **novpax.org** is canonical (GitHub Pages atmospeak site is transitional only) |
| Free vs Pro | Separate binaries and policies |
| Free | Public MIT dictation; no account; public CDN |
| Pro | Separate build; **Polar online** licence; **gated** updates; $69 one-time; **3 years** of Pro updates; lifetime use of installed Pro after window |
| Naming | **Atmospeak** absorbs **Atmos** (meeting transcription = future Pro capability, not a second waitlist product) |
| vs Sanctuary | Sanctuary stays subscription + account; Atmospeak Pro stays one-time + Polar licence — intentional |

---

## What Atmospeak already did (assume done)

1. Free updater points at `https://downloads.novpax.org/atmospeak/free/latest.json`
2. Pro build: `cargo --features pro` + `tauri.pro.conf.json`; updater host `https://updates.novpax.org/atmospeak/pro/latest.json`
3. Polar activate/validate client + 14-day offline grace in Pro app
4. First Pro features: airplane mode + network ledger (`src-pro/`)
5. Gated update Worker source in `services/pro-updates/` (HMAC artifact URLs after Polar validate)
6. Placeholder Buy URL still `https://buy.polar.sh/polar_cl_REPLACE` — **you must replace with real checkout**
7. Website mirror in atmospeak `website/` already links to novpax + free CDN; canonical tag → `https://www.novpax.org/projects/atmospeak`

Current free version string in that repo: **1.0.3**  
Free installer name pattern: `atmospeak_1.0.3_x64-setup.exe`  
Pro installer name pattern: `atmospeak-pro_<version>_x64-setup.exe`

---

## Your deliverables (do in order)

### 1. Product page on novpax.org

Create / replace page at **`/projects/atmospeak`** (or equivalent route that matches `https://www.novpax.org/projects/atmospeak`).

**Projects / roadmap list**

- Remove or replace the dormant **Atmos** “local AI meeting transcription / waitlist” card.
- List **Atmospeak** as live: free local dictation on Windows; Pro separate.
- Meetings = Atmospeak Pro roadmap item (not a separate product).

**Page content**

- Brand: Atmospeak (Nov Pax)
- Hero: local dictation you own; free MIT surface; Pro = separate licensed build
- CTA **Download free** → public CDN (see URLs below). Prefer versioned NSIS setup EXE; also link checksums.
- CTA **Buy Pro — $69** → real Polar checkout link
- Short FAQ: why Sanctuary is subscription but Atmospeak Pro is one-time; what “3 years of updates / lifetime after” means (no new Pro builds after window; installed Pro keeps working)
- Do **not** imply Pro requires a Sanctuary account

Match existing Nov Pax visual language; don’t invent a second design system.

### 2. Polar (same org as Sanctuary)

Configure the existing **$69** Atmospeak Pro one-time product:

1. **License Keys** benefit  
   - Optional prefix `ATMO`  
   - Prefer **no short key expiry** (update window is enforced by our Worker, not by killing the key)  
   - Activations: recommend limit **3**  
   - Record `benefit_id` and `organization_id` for secrets / handback to Atmospeak `.env`

2. **File Downloads** benefit  
   - Placeholder until first Pro installer exists; document where CI will refresh `atmospeak-pro_*_x64-setup.exe`

3. **Checkout link**  
   - Create public checkout URL  
   - Wire Buy buttons on novpax.org to it  
   - Hand back the URL so Atmospeak can replace `polar_cl_REPLACE` in `website/src/version.ts` and app env `ATMOSPEAK_POLAR_CHECKOUT_URL`

### 3. Cloudflare hosting

**A. Free public CDN — `downloads.novpax.org`**

- Host prefix: `/atmospeak/free/`
- Public objects (after each free release from Atmospeak CI / manual upload):
  - `latest.json`
  - `atmospeak_<ver>_x64-setup.exe` (+ `.sig` if present)
  - MSI / portable / `SHA256SUMS.txt` as published
- CORS not critical for browser downloads; updater only needs HTTPS GET
- Optional: R2 bucket + custom domain `downloads.novpax.org`

**B. Pro private + Worker — `updates.novpax.org`**

- Private R2 bucket (suggested name `atmospeak-pro`)
- Deploy Worker from Atmospeak repo: `services/pro-updates/` (`wrangler.toml` already sketches route `updates.novpax.org/atmospeak/pro/*`)
- Secrets:
  - `POLAR_ACCESS_TOKEN` (server-only)
  - `POLAR_ORGANIZATION_ID`
  - `POLAR_LICENSE_BENEFIT_ID` (optional but recommended)
  - `ARTIFACT_SIGNING_SECRET`
- Vars: `PRO_MANIFEST_KEY` default `atmospeak/pro/latest.json`; `UPDATE_WINDOW_YEARS=3`
- Object layout:
  - `atmospeak/pro/latest.json`
  - `atmospeak/pro/artifacts/<installer filename>`
- Worker behaviour (already coded): validate Polar via headers `X-Atmospeak-License` + `X-Atmospeak-Activation`; refuse if outside 3-year window; return `latest.json` with HMAC-signed artifact URLs

**C. DNS**

- `downloads.novpax.org` → free CDN
- `updates.novpax.org` → Pro Worker

### 4. Cross-links / retirement of placeholders

- From novpax.org Atmospeak page: free download + Buy Pro working end-to-end (even if Pro installer is “coming soon” until first Pro release is uploaded)
- Optionally add Atmospeak to site nav / pricing overview with a clear “different commerce model” note
- If you control redirects: GitHub Pages `leviathofnoesia.github.io/atmospeak` → `https://www.novpax.org/projects/atmospeak` (Atmospeak repo already sets `rel=canonical`)

### 5. Hand back to Atmospeak (write a short report)

When done (or blocked), report:

1. Live product page URL
2. Polar checkout URL
3. `POLAR_ORGANIZATION_ID` + `POLAR_LICENSE_BENEFIT_ID` (ids only; not access tokens)
4. Whether `downloads.novpax.org/atmospeak/free/` serves objects (or still empty)
5. Whether `updates.novpax.org` Worker is deployed
6. Any secrets Atmospeak Pro CI still needs
7. Blockers you could not finish (Authenticode, missing Pro binary, etc.)

---

## URL contract (must match Atmospeak builds)

| Purpose | URL |
| --- | --- |
| Free latest.json | `https://downloads.novpax.org/atmospeak/free/latest.json` |
| Free setup (example v1.0.3) | `https://downloads.novpax.org/atmospeak/free/atmospeak_1.0.3_x64-setup.exe` |
| Pro latest.json (auth headers) | `https://updates.novpax.org/atmospeak/pro/latest.json` |
| Product page | `https://www.novpax.org/projects/atmospeak` |
| Buy | Polar checkout (you create) |

---

## Explicit non-goals for this handoff

- Do not put Atmospeak Pro behind Sanctuary login / suite SSO
- Do not make Pro a monthly subscription to “match” Sanctuary pricing UI
- Do not publish Pro `latest.json` or Pro installers on a public unauthenticated URL
- Do not paywall the free dictation surface
- Do not invent a second product named Atmos

---

## Success criteria

- [ ] `/projects/atmospeak` live; Atmos waitlist gone; meetings framed under Atmospeak Pro roadmap
- [ ] Download free CTA hits `downloads.novpax.org` (or clearly “artifacts pending upload” with correct final URL)
- [ ] Buy Pro CTA hits working Polar checkout for the $69 product with licence + file-download benefits configured
- [ ] DNS + R2 + Worker for free CDN and gated Pro updates either live or fully scripted with deploy steps
- [ ] Written handback with Polar ids + checkout URL for the Atmospeak repo to finish env wiring

Start by inspecting the current novpax.org projects/pricing routes and Polar product config, then implement the page and infra in that order.
