#Requires -Version 5.1
# Build the pinned transcribe.cpp CPU sidecar. Optionally stage Parakeet for smoke tests.
[CmdletBinding()]
param(
  [string]$BuildRoot = $(if ($env:ATMOSPEAK_ASR_BUILD_ROOT) { $env:ATMOSPEAK_ASR_BUILD_ROOT } else { "E:\BuildCache\atmospeak-asr-build" }),
  [string]$SelectionRoot = $(if ($env:ATMOSPEAK_ASR_SELECTION_ROOT) { $env:ATMOSPEAK_ASR_SELECTION_ROOT } else { "E:\BuildCache\atmospeak-asr-cache\parakeet" }),
  [switch]$SkipModel
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Manifest = Join-Path $Root "src-asr-host\Cargo.toml"
$OutputDir = Join-Path $Root "src-tauri\resources\asr"
$DefaultTempRoot = Join-Path $BuildRoot "tmp"
$DefaultTargetDir = Join-Path $BuildRoot "cargo-target"
$DefaultCargoHome = "E:\BuildCache\cargo"

function Assert-EPath {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string]$Label
  )
  try {
    $full = [System.IO.Path]::GetFullPath($Path)
  } catch {
    throw "$Label is not a valid path: $Path"
  }
  if ($full -notmatch "^[Ee]:\\") {
    throw "$Label must resolve under E: for an isolated build: $full"
  }
  return $full
}

$BuildRoot = Assert-EPath $BuildRoot "ASR build root"
if (-not $SkipModel) {
  $SelectionRoot = Assert-EPath $SelectionRoot "ASR selection cache"
}
$TempRoot = Assert-EPath $(if ($env:ATMOSPEAK_BUILD_TEMP) { $env:ATMOSPEAK_BUILD_TEMP } else { $DefaultTempRoot }) "ATMOSPEAK_BUILD_TEMP"
$TargetDir = Assert-EPath $(if ($env:ATMOSPEAK_ASR_TARGET_DIR) { $env:ATMOSPEAK_ASR_TARGET_DIR } else { $DefaultTargetDir }) "ATMOSPEAK_ASR_TARGET_DIR"
$CargoHome = Assert-EPath $(if ($env:CARGO_HOME) { $env:CARGO_HOME } else { $DefaultCargoHome }) "CARGO_HOME"
$OutputDir = Assert-EPath $OutputDir "ASR resource output"

$directories = @($BuildRoot, $TempRoot, $TargetDir, $CargoHome, $OutputDir)
if (-not $SkipModel) { $directories += $SelectionRoot }
foreach ($directory in $directories) {
  New-Item -ItemType Directory -Force -Path $directory | Out-Null
}
$env:TEMP = $TempRoot
$env:TMP = $TempRoot
$env:CARGO_HOME = $CargoHome
$env:CARGO_TARGET_DIR = $TargetDir
$env:ATMOSPEAK_ASR_BUILD_ROOT = $BuildRoot

if (-not $SkipModel) {
  $modelName = 'parakeet-unified-en-0.6b-Q8_0.gguf'
  $modelHash = '4b50b6dd862bf6e346929aaf4f5eaacec003bfa3f56462d6c874b41ef2f38795'
  $cached = Join-Path $SelectionRoot $modelName
  if (-not (Test-Path -LiteralPath $cached -PathType Leaf) -or
      (Get-Item -LiteralPath $cached).Length -ne 731357568 -or
      (Get-FileHash -LiteralPath $cached -Algorithm SHA256).Hash -ne $modelHash) {
    $partial = "$cached.$PID.partial"
    try {
      $modelUrl = "https://huggingface.co/handy-computer/parakeet-unified-en-0.6b-gguf/resolve/7e948f21b7bdbac698d3318db9d350f1096f3b6c/$modelName"
      & curl.exe --fail --location --retry 2 --max-time 900 --output $partial $modelUrl
      if ($LASTEXITCODE -ne 0) { throw "Parakeet download failed (exit $LASTEXITCODE)." }
      if ((Get-Item -LiteralPath $partial).Length -ne 731357568 -or
          (Get-FileHash -LiteralPath $partial -Algorithm SHA256).Hash -ne $modelHash) {
        throw "Parakeet weights do not match the pinned artifact."
      }
      Move-Item -LiteralPath $partial -Destination $cached -Force
    } finally {
      if (Test-Path -LiteralPath $partial) { Remove-Item -LiteralPath $partial -Force }
    }
  }
}

$env:TRANSCRIBE_CMAKE_ARGS = "-DGGML_NATIVE=OFF"
$messages = & cargo build --locked --release --manifest-path $Manifest --message-format=json
if ($LASTEXITCODE -ne 0) {
  $messages | ForEach-Object {
    $record = ConvertFrom-Json $_
    if ($record.reason -eq 'compiler-message') { Write-Host $record.message.rendered }
  }
  throw "CPU ASR sidecar build failed (exit $LASTEXITCODE)."
}
$nativeBuild = $messages | ForEach-Object { ConvertFrom-Json $_ } | Where-Object {
  $_.reason -eq 'build-script-executed' -and $_.package_id -like '*#transcribe-cpp-sys@*'
} | Select-Object -Last 1
if (-not $nativeBuild) { throw "Cargo did not report the pinned transcribe.cpp runtime." }
$nativeDir = Join-Path $nativeBuild.out_dir 'bin'
$tauri = Get-Content -LiteralPath (Join-Path $Root "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json
$asrNames = @($tauri.bundle.resources | Where-Object { $_ -like 'resources/asr/*.dll' } | Split-Path -Leaf)
$crtNames = @('MSVCP140.dll', 'MSVCP140_1.dll', 'VCRUNTIME140.dll', 'VCRUNTIME140_1.dll')
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$visualStudio = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $visualStudio) { throw "Visual Studio C++ Build Tools and their x64 redistributable files are required." }
$redist = Get-ChildItem -Path (Join-Path $visualStudio 'VC/Redist/MSVC/*/x64/Microsoft.VC*.CRT') -Directory |
  Sort-Object { [version]$_.Parent.Parent.Name } -Descending | Where-Object {
    $candidate = $_.FullName
    @($crtNames | Where-Object { -not (Test-Path -LiteralPath (Join-Path $candidate $_) -PathType Leaf) }).Count -eq 0
  } | Select-Object -First 1
if (-not $redist) { throw "Visual Studio's x64 C++ redistributable files are missing." }
foreach ($name in $asrNames) {
  $directory = if ($name -in $crtNames) { $redist.FullName } else { $nativeDir }
  Copy-Item -LiteralPath (Join-Path $directory $name) -Destination (Join-Path $OutputDir $name) -Force
}
Copy-Item -LiteralPath (Join-Path $TargetDir "release\atmospeak-asr-host.exe") -Destination (Join-Path $OutputDir "atmospeak-asr-cpu.exe") -Force
if (-not $SkipModel) {
  $modelOutput = Join-Path $Root "src-tauri\resources\models"
  New-Item -ItemType Directory -Force -Path $modelOutput | Out-Null
  Copy-Item -LiteralPath $cached -Destination (Join-Path $modelOutput $modelName) -Force
}
Write-Host "CPU speech sidecar staged."
