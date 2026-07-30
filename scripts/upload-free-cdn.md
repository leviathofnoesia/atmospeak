# Upload free release artifacts to downloads.novpax.org

Requires Cloudflare credentials with write access to the public downloads bucket
(or whatever backs `downloads.novpax.org`).

Example with Wrangler R2 (adjust bucket / prefix to match DNS):

```powershell
$ReleaseDir = "release\free"
bunx wrangler r2 object put atmospeak-downloads/atmospeak/free/latest.json --file "$ReleaseDir\latest.json"
Get-ChildItem $ReleaseDir -File | ForEach-Object {
  bunx wrangler r2 object put "atmospeak-downloads/atmospeak/free/$($_.Name)" --file $_.FullName
}
```

Pro artifacts go to the **private** `atmospeak-pro` bucket — see
`services/pro-updates/README.md`.
