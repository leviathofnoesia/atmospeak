param(
  [switch]$CpuOnly
)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
$Manifest = Join-Path $Root "src-asr-host\Cargo.toml"
$TargetDir = if ($env:ATMOSPEAK_ASR_TARGET_DIR) {
  $env:ATMOSPEAK_ASR_TARGET_DIR
} else {
  Join-Path $Root "src-asr-host\target"
}
$OutputDir = Join-Path $Root "src-tauri\resources\asr"
$VadPath = Join-Path $OutputDir "ggml-silero-v6.2.0.bin"
$VadUrl = "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin"
$VadSha256 = "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987"

if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
  throw "CMake is required to build atmospeak-asr-host. Install CMake and place it on PATH."
}
if (-not $env:LIBCLANG_PATH -and -not (Get-Command clang -ErrorAction SilentlyContinue)) {
  throw "LLVM/libclang is required by whisper-rs. Set LIBCLANG_PATH or install LLVM."
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
if (-not (Test-Path -LiteralPath $VadPath)) {
  Invoke-WebRequest -Uri $VadUrl -OutFile $VadPath
}
$ActualVadSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $VadPath).Hash.ToLowerInvariant()
if ($ActualVadSha256 -ne $VadSha256) {
  throw "Silero VAD checksum mismatch: expected $VadSha256, received $ActualVadSha256"
}

$env:CARGO_TARGET_DIR = $TargetDir
cargo build --locked --release --manifest-path $Manifest
if ($LASTEXITCODE -ne 0) {
  throw "CPU ASR sidecar build failed with exit code $LASTEXITCODE"
}
Copy-Item `
  (Join-Path $TargetDir "release\atmospeak-asr-host.exe") `
  (Join-Path $OutputDir "atmospeak-asr-cpu.exe") `
  -Force

if (-not $CpuOnly) {
  if (-not $env:VULKAN_SDK) {
    throw "VULKAN_SDK is required for the Vulkan ASR sidecar."
  }
  cargo build --locked --release --features vulkan --manifest-path $Manifest
  if ($LASTEXITCODE -ne 0) {
    throw "Vulkan ASR sidecar build failed with exit code $LASTEXITCODE"
  }
  Copy-Item `
    (Join-Path $TargetDir "release\atmospeak-asr-host.exe") `
    (Join-Path $OutputDir "atmospeak-asr-vulkan.exe") `
    -Force
}

Write-Host "ASR sidecars are ready in $OutputDir"
