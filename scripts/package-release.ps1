#Requires -Version 5.1
param(
  [ValidateSet("free")]
  [string]$Channel = "free",
  [string]$ReleaseRepo = $env:ATMOSPEAK_RELEASE_REPO,
  [string]$FreeCdnBase = $env:ATMOSPEAK_FREE_CDN_BASE,
  [switch]$SkipTauriBuild
)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($ReleaseRepo)) {
  $ReleaseRepo = "leviathofnoesia/atmospeak"
}
if ([string]::IsNullOrWhiteSpace($FreeCdnBase)) {
  $FreeCdnBase = "https://www.novpax.org/downloads/atmospeak/free"
}
$FreeCdnBase = $FreeCdnBase.TrimEnd("/")

$Package = Get-Content (Join-Path $Root "package.json") -Raw | ConvertFrom-Json
$Version = [string]$Package.version
$ReleaseDir = Join-Path $Root "release\free"
$StageDir = Join-Path $ReleaseDir "portable-stage"
# Honor CARGO_TARGET_DIR (Cursor sandboxes redirect here). Without this, the
# script packages stale installers from src-tauri/target while the real build
# landed elsewhere.
$CargoTargetRoot = if (-not [string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
  Join-Path $env:CARGO_TARGET_DIR "release"
} else {
  Join-Path $Root "src-tauri\target\release"
}
$BundleRoot = Join-Path $CargoTargetRoot "bundle"
$ReleaseExe = Join-Path $CargoTargetRoot "atmospeak.exe"
$SidecarExe = Join-Path $CargoTargetRoot "whisper-cli.exe"
$ResourcesDir = Join-Path $CargoTargetRoot "resources"
$DefaultKey = Join-Path $env:USERPROFILE ".tauri\atmospeak\updater.key"
$CargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
$TauriCli = Join-Path $Root "node_modules\.bin\tauri.exe"
$SigningKeyPath = $env:TAURI_SIGNING_PRIVATE_KEY_PATH
if ([string]::IsNullOrWhiteSpace($SigningKeyPath) -and (Test-Path $DefaultKey)) {
  $SigningKeyPath = $DefaultKey
}
$HasUpdaterSigningKey = $false
if ($env:TAURI_SIGNING_PRIVATE_KEY -or -not [string]::IsNullOrWhiteSpace($SigningKeyPath)) {
  $HasUpdaterSigningKey = $true
}

function Invoke-TauriBuild([string[]]$ExtraArgs) {
  if (Test-Path $TauriCli) {
    & $TauriCli build @ExtraArgs
  } else {
    bunx tauri build @ExtraArgs
  }
}

function Invoke-UpdaterSigner([string]$FilePath) {
  if ($env:TAURI_SIGNING_PRIVATE_KEY) {
    & bunx tauri signer sign $FilePath
    return
  }
  if (-not [string]::IsNullOrWhiteSpace($SigningKeyPath)) {
    $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content $SigningKeyPath -Raw
    try {
      & bunx tauri signer sign $FilePath
    } finally {
      Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
    }
  }
}

$env:CI = "true"
if ((Test-Path $CargoBin) -and ($env:PATH -notlike "*$CargoBin*")) {
  $env:PATH = "$CargoBin;$env:PATH"
}

$unsignedConfigPath = $null
if (-not $SkipTauriBuild) {
  Push-Location $Root
  try {
    $previousCargoTargetDir = $env:CARGO_TARGET_DIR
    $sidecarExitCode = 0
    try {
      & (Join-Path $PSScriptRoot "build-asr-sidecars.ps1")
      $sidecarExitCode = $LASTEXITCODE
    } finally {
      if ($null -eq $previousCargoTargetDir) {
        Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
      } else {
        $env:CARGO_TARGET_DIR = $previousCargoTargetDir
      }
    }
    if ($sidecarExitCode -ne 0) {
      throw "ASR sidecar build failed with exit code $sidecarExitCode"
    }
    $buildArgs = @()
    if (-not $HasUpdaterSigningKey) {
      Write-Warning "No Tauri updater signing key found. Building unsigned local installers without updater artifacts or latest.json."
      $unsignedConfigPath = Join-Path $env:TEMP "atmospeak-tauri-unsigned-build.json"
      [System.IO.File]::WriteAllText(
        $unsignedConfigPath,
        "{`"bundle`":{`"createUpdaterArtifacts`":false}}",
        [System.Text.UTF8Encoding]::new($false)
      )
      $buildArgs += @("--config", $unsignedConfigPath)
    }

    Invoke-TauriBuild $buildArgs
    if ($LASTEXITCODE -ne 0) {
      throw "Tauri build failed with exit code $LASTEXITCODE"
    }
    $channelStamp = Join-Path $CargoTargetRoot "atmospeak-build-channel.txt"
    [System.IO.File]::WriteAllText(
      $channelStamp,
      $Channel,
      [System.Text.UTF8Encoding]::new($false)
    )
  } finally {
    if ($unsignedConfigPath -and (Test-Path $unsignedConfigPath)) {
      Remove-Item $unsignedConfigPath -Force -ErrorAction SilentlyContinue
    }
    Pop-Location
  }
} else {
  $channelStamp = Join-Path $CargoTargetRoot "atmospeak-build-channel.txt"
  if (-not (Test-Path $channelStamp)) {
    throw "SkipTauriBuild requires $channelStamp from a prior full build of channel '$Channel'."
  }
  $stamped = (Get-Content $channelStamp -Raw).Trim()
  if ($stamped -ne $Channel) {
    throw "SkipTauriBuild channel mismatch: stamp='$stamped' but -Channel $Channel. Rebuild without -SkipTauriBuild."
  }
}

if (-not (Test-Path $BundleRoot)) {
  throw "Missing bundle output: $BundleRoot"
}

$NsisSource = Get-ChildItem (Join-Path $BundleRoot "nsis") -Filter "*$Version*.exe" -File |
  Where-Object { $_.Name -notlike "*.sig" } |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1
$MsiSource = Get-ChildItem (Join-Path $BundleRoot "msi") -Filter "*$Version*.msi" -File |
  Where-Object { $_.Name -notlike "*.sig" } |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1
if (-not $NsisSource) { throw "NSIS installer for version $Version was not produced under $BundleRoot\nsis." }
if (-not $MsiSource) { throw "MSI installer for version $Version was not produced under $BundleRoot\msi." }

$ProductSlug = "atmospeak"
$NsisName = "$ProductSlug`_$Version`_x64-setup.exe"
$MsiName = "$ProductSlug`_$Version`_x64_en-US.msi"
$UpdaterZipName = "$ProductSlug`_$Version`_x64-setup.nsis.zip"
$PortableName = "$ProductSlug`_$Version`_x64-portable.zip"
$NsisDest = Join-Path $ReleaseDir $NsisName
$MsiDest = Join-Path $ReleaseDir $MsiName
$UpdaterZipDest = Join-Path $ReleaseDir $UpdaterZipName
$PortableDest = Join-Path $ReleaseDir $PortableName
$LatestName = "latest.json"
$LatestDest = Join-Path $ReleaseDir $LatestName

New-Item -ItemType Directory -Force -Path $ReleaseDir | Out-Null
Copy-Item $NsisSource.FullName $NsisDest -Force
Copy-Item $MsiSource.FullName $MsiDest -Force

$UpdaterAssetName = $NsisName
$UpdaterSignaturePath = "$NsisDest.sig"
if ($HasUpdaterSigningKey) {
  $UpdaterSource = Get-ChildItem (Join-Path $BundleRoot "nsis") -Filter "*.nsis.zip" -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
  if ($UpdaterSource) {
    $UpdaterAssetName = $UpdaterZipName
    $UpdaterSignaturePath = "$UpdaterZipDest.sig"
    Copy-Item $UpdaterSource.FullName $UpdaterZipDest -Force
    $sigBeside = "$($UpdaterSource.FullName).sig"
    if (Test-Path $sigBeside) {
      Copy-Item $sigBeside $UpdaterSignaturePath -Force
    }
  } else {
    Compress-Archive -Path $NsisDest -DestinationPath $UpdaterZipDest -CompressionLevel Optimal
    Invoke-UpdaterSigner $UpdaterZipDest
    if (-not (Test-Path "$UpdaterZipDest.sig")) {
      throw "Failed to sign $UpdaterZipDest"
    }
    $UpdaterAssetName = $UpdaterZipName
    $UpdaterSignaturePath = "$UpdaterZipDest.sig"
  }
  if (-not (Test-Path $UpdaterSignaturePath)) {
    # Fall back to signing the NSIS exe when zip.sig is missing.
    Invoke-UpdaterSigner $NsisDest
    $UpdaterAssetName = $NsisName
    $UpdaterSignaturePath = "$NsisDest.sig"
  }
}

# Portable zip
if (Test-Path $StageDir) { Remove-Item -Recurse -Force $StageDir }
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
Copy-Item $ReleaseExe (Join-Path $StageDir "atmospeak.exe") -Force
if (Test-Path $SidecarExe) {
  Copy-Item $SidecarExe (Join-Path $StageDir "whisper-cli.exe") -Force
}
if (Test-Path $ResourcesDir) {
  Copy-Item $ResourcesDir (Join-Path $StageDir "resources") -Recurse -Force
}
if (Test-Path $PortableDest) { Remove-Item $PortableDest -Force }
Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $PortableDest -CompressionLevel Optimal
Remove-Item -Recurse -Force $StageDir

# Checksums
Get-FileHash $NsisDest -Algorithm SHA256 | ForEach-Object { "$($_.Hash.ToLower())  $NsisName" } |
  Set-Content (Join-Path $ReleaseDir "$NsisName.sha256") -Encoding ascii
Get-FileHash $MsiDest -Algorithm SHA256 | ForEach-Object { "$($_.Hash.ToLower())  $MsiName" } |
  Set-Content (Join-Path $ReleaseDir "$MsiName.sha256") -Encoding ascii
Get-FileHash $PortableDest -Algorithm SHA256 | ForEach-Object { "$($_.Hash.ToLower())  $PortableName" } |
  Set-Content (Join-Path $ReleaseDir "$PortableName.sha256") -Encoding ascii

if ($HasUpdaterSigningKey -and (Test-Path $UpdaterSignaturePath)) {
  $signature = (Get-Content $UpdaterSignaturePath -Raw).Trim()
  $UpdaterUrl = "$FreeCdnBase/$UpdaterAssetName"
  $notes = "Atmospeak free $Version"
  $latest = [ordered]@{
    version = $Version
    notes   = $notes
    pub_date = (Get-Date).ToUniversalTime().ToString("o")
    platforms = [ordered]@{
      "windows-x86_64" = [ordered]@{
        signature = $signature
        url = $UpdaterUrl
      }
    }
  }
  ($latest | ConvertTo-Json -Depth 6) | Set-Content $LatestDest -Encoding utf8
}

Write-Host "Free release artifacts in $ReleaseDir"
Write-Host "Upload installers first, then latest.json last — see scripts/upload-free-cdn.md"
Write-Host "Pro builds: private Nov-Pax-Web products/atmospeak-pro (scripts/package-pro.ps1)."
