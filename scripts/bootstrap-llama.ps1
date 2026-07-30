param(
  [string]$Tag = "b10178",
  [string]$RuntimeZip = "llama-b10178-bin-win-cpu-x64.zip",
  # Keep in sync with LLAMA_RUNTIME_ZIP_SHA256 / LLAMA_SERVER_EXE_SHA256 in
  # src-tauri/src/services/llama_host.rs
  [string]$ExpectedZipSha256 = "55e419591f9798e1ffe6ec3a088cf162a93c07f8a7c8e0fc5b8bf9948155e1b1",
  [string]$ExpectedServerSha256 = "cfbed03a6f7a904ed06e385304dcffc9c64897c3057906b05b7fbd4b62956961"
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$tmp = Join-Path $root ".tmp\llama"
$runtime = Join-Path $root "src-tauri\resources\llama-runtime"

New-Item -ItemType Directory -Force -Path $tmp, $runtime | Out-Null

$zip = Join-Path $tmp $RuntimeZip
$runtimeUrl = "https://github.com/ggml-org/llama.cpp/releases/download/$Tag/$RuntimeZip"

curl.exe -L -o $zip $runtimeUrl

$zipHash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
if ($zipHash -ne $ExpectedZipSha256.ToLowerInvariant()) {
  throw "llama runtime zip checksum mismatch: expected $ExpectedZipSha256, got $zipHash"
}

Expand-Archive -Path $zip -DestinationPath (Join-Path $tmp "bin") -Force

$server = Get-ChildItem -Path (Join-Path $tmp "bin") -Recurse -Filter "llama-server.exe" |
  Select-Object -First 1
if (-not $server) {
  throw "llama-server.exe not found in $RuntimeZip"
}

$serverHash = (Get-FileHash -Algorithm SHA256 $server.FullName).Hash.ToLowerInvariant()
if ($serverHash -ne $ExpectedServerSha256.ToLowerInvariant()) {
  throw "llama-server.exe checksum mismatch: expected $ExpectedServerSha256, got $serverHash"
}

Copy-Item $server.FullName (Join-Path $runtime "llama-server.exe") -Force
Set-Content -Path (Join-Path $runtime "llama-server.exe.sha256") -Value $ExpectedServerSha256

# Copy sibling DLLs if present (some builds are self-contained).
Get-ChildItem -Path $server.DirectoryName -Filter "*.dll" | ForEach-Object {
  Copy-Item $_.FullName $runtime -Force
}

Write-Host "Bundled llama-server refreshed from $runtimeUrl"
Write-Host "Installed to $runtime (zip=$zipHash server=$serverHash)"
