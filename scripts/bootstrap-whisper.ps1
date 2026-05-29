param(
  [string]$Version = "v1.8.4",
  [string]$RuntimeZip = "whisper-bin-x64.zip"
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$tmp = Join-Path $root ".tmp\whisper"
$release = Join-Path $tmp "bin\Release"
$binaries = Join-Path $root "src-tauri\binaries"
$runtime = Join-Path $root "src-tauri\resources\whisper-runtime"
$models = Join-Path $root "src-tauri\resources\models"

New-Item -ItemType Directory -Force -Path $tmp, $binaries, $runtime, $models | Out-Null

$zip = Join-Path $tmp $RuntimeZip
$runtimeUrl = "https://github.com/ggml-org/whisper.cpp/releases/download/$Version/$RuntimeZip"
$modelUrl = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin"

curl.exe -L -o $zip $runtimeUrl
Expand-Archive -Path $zip -DestinationPath (Join-Path $tmp "bin") -Force

Copy-Item (Join-Path $release "whisper-cli.exe") (Join-Path $binaries "whisper-cli-x86_64-pc-windows-msvc.exe") -Force
Copy-Item @(
  (Join-Path $release "whisper-cli.exe"),
  (Join-Path $release "whisper.dll"),
  (Join-Path $release "ggml.dll"),
  (Join-Path $release "ggml-base.dll"),
  (Join-Path $release "ggml-cpu.dll")
) $runtime -Force

curl.exe -L -o (Join-Path $models "ggml-base.en.bin") $modelUrl

Write-Host "Bundled whisper.cpp runtime refreshed from $runtimeUrl"
Write-Host "Bundled model refreshed from $modelUrl"
