param(
  [Parameter(Mandatory = $true)][string]$AppExe,
  [Parameter(Mandatory = $true)][string]$Recording,
  [Parameter(Mandatory = $true)][string]$SeedDatabase,
  [Parameter(Mandatory = $true)][string]$RunRoot,
  [string]$ModelPath = (Join-Path $PSScriptRoot "../src-tauri/resources/models/parakeet-unified-en-0.6b-Q8_0.gguf"),
  [ValidateRange(1, 30)][int]$Runs = 5,
  [switch]$AllowDesktopInput
)

$ErrorActionPreference = "Stop"
if (-not $AllowDesktopInput) { throw "Desktop input requires explicit permission for an uninterrupted session. This tool must not run while Billy is working." }
foreach ($entry in @($AppExe, $Recording, $SeedDatabase, $ModelPath, $RunRoot)) {
  if ([IO.Path]::GetFullPath($entry) -notmatch "^[Ee]:\\") {
    throw "Native replay inputs and output must stay on E: $entry"
  }
}
$node = $env:ATMOSPEAK_NODE_EXE
if ([string]::IsNullOrWhiteSpace($node)) {
  $node = Join-Path $env:USERPROFILE ".cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin\node.exe"
}
if ((& $node --version) -notmatch "^v24\.") { throw "Use Node 24 for native SQLite readback." }
$env:TEMP = Join-Path $RunRoot "tmp"
$env:TMP = $env:TEMP
$env:NODE_COMPILE_CACHE = Join-Path $RunRoot "node-cache"
New-Item -ItemType Directory -Force $env:TEMP | Out-Null
# Use an optimized replay build with debug assertions enabled for the existing
# saved-audio input. This exercises native shortcuts and paste without a mic,
# development server, remote debugging port, or WebView automation.
& $node (Join-Path $PSScriptRoot "native-paste-latency-harness.mjs") "--app=$AppExe" "--recording=$Recording" "--seed-database=$SeedDatabase" "--model=$ModelPath" "--run-root=$RunRoot" "--runs=$Runs" "--allow-desktop-input=true"
if ($LASTEXITCODE -ne 0) { throw "Native Submit-to-text measurement failed ($LASTEXITCODE)." }
