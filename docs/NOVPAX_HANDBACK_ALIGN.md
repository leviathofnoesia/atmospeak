# Nov Pax handback alignment (Atmospeak side)

Revised storefront contract from the novpax.org project. Prefer this over older
`downloads.novpax.org` wording in earlier handoff drafts.

## Locked free CDN

| Purpose | URL |
| --- | --- |
| Free base | `https://www.novpax.org/downloads/atmospeak/free` |
| Free latest.json | `https://www.novpax.org/downloads/atmospeak/free/latest.json` |
| Free setup (example) | `https://www.novpax.org/downloads/atmospeak/free/atmospeak_1.0.3_x64-setup.exe` |
| Product page | `https://www.novpax.org/projects/atmospeak` |
| Buy Pro | `https://buy.polar.sh/polar_cl_a4ccEACEPXCGN3mZk0JDyDVXzeob3K8CSpYeB34qQsE` |

- Host files in novpax.org `public/downloads/atmospeak/free/` (Sanctuary Desktop pattern).
- **Do not** create `downloads.novpax.org`.

## Polar (production)

| Field | Value |
| --- | --- |
| organization_id | `97f0d813-d25f-4cc4-b934-fd4705a01c47` |
| product_id | `184d5727-0fbe-475c-b6c0-32fd93461cf9` |
| license benefit_id | `b4e88474-01fa-450c-9aac-07bd92d8e887` |
| Checkout | see Buy Pro URL above |

File Downloads benefit: deferred until first Pro EXE exists.

## Pro gated Worker

`updates.novpax.org` / Nov-Pax-Web `products/atmospeak-pro/services/pro-updates/`
remains a Cloudflare follow-up. Not blocking free CDN or Polar checkout.

## Atmospeak repo status

App + packaging + website defaults already point at the www free path and real
Polar checkout (see `.env.example`, `tauri.conf.json`, `website/src/version.ts`).
