param(
  [string]$Tag = "b10178",
  [string]$RuntimeZip = "llama-b10178-bin-win-cpu-x64.zip"
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$tmp = Join-Path $root ".tmp\llama"
$runtime = Join-Path $root "src-tauri\resources\llama-runtime"

New-Item -ItemType Directory -Force -Path $tmp, $runtime | Out-Null

$zip = Join-Path $tmp $RuntimeZip
$runtimeUrl = "https://github.com/ggml-org/llama.cpp/releases/download/$Tag/$RuntimeZip"

curl.exe -L -o $zip $runtimeUrl
Expand-Archive -Path $zip -DestinationPath (Join-Path $tmp "bin") -Force

$server = Get-ChildItem -Path (Join-Path $tmp "bin") -Recurse -Filter "llama-server.exe" |
  Select-Object -First 1
if (-not $server) {
  throw "llama-server.exe not found in $RuntimeZip"
}

Copy-Item $server.FullName (Join-Path $runtime "llama-server.exe") -Force

# Copy sibling DLLs if present (some builds are self-contained).
Get-ChildItem -Path $server.DirectoryName -Filter "*.dll" | ForEach-Object {
  Copy-Item $_.FullName $runtime -Force
}

Write-Host "Bundled llama-server refreshed from $runtimeUrl"
Write-Host "Installed to $runtime"
