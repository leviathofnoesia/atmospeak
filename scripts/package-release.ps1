param(
  [ValidateSet("free", "pro")]
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
$IsPro = $Channel -eq "pro"

$Package = Get-Content (Join-Path $Root "package.json") -Raw | ConvertFrom-Json
$Version = [string]$Package.version
$ReleaseDir = if ($IsPro) {
  Join-Path $Root "release\pro"
} else {
  Join-Path $Root "release\free"
}
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

function Get-Sha256Hex([string]$Path) {
  $stream = [System.IO.File]::OpenRead($Path)
  try {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
      $bytes = $sha.ComputeHash($stream)
      return ([BitConverter]::ToString($bytes)).Replace("-", "").ToLowerInvariant()
    } finally {
      $sha.Dispose()
    }
  } finally {
    $stream.Dispose()
  }
}

function Invoke-UpdaterSigner([string]$Path) {
  $signerCommand = "bun"
  $signerPrefix = @("tauri")
  if (Test-Path $TauriCli) {
    $signerCommand = $TauriCli
    $signerPrefix = @()
  }

  if (-not [string]::IsNullOrWhiteSpace($SigningKeyPath)) {
    $savedPrivateKey = $env:TAURI_SIGNING_PRIVATE_KEY
    Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
    try {
      & $signerCommand @signerPrefix signer sign -f $SigningKeyPath $Path
    } finally {
      if ($null -ne $savedPrivateKey) {
        $env:TAURI_SIGNING_PRIVATE_KEY = $savedPrivateKey
      }
    }
    return
  }

  if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
    throw "Missing updater signing key. Set TAURI_SIGNING_PRIVATE_KEY, TAURI_SIGNING_PRIVATE_KEY_PATH, or create $DefaultKey."
  }
  & $signerCommand @signerPrefix signer sign -k $env:TAURI_SIGNING_PRIVATE_KEY $Path
}

function Invoke-TauriBuild {
  param([string[]]$BuildArgs)

  if (Test-Path $TauriCli) {
    & $TauriCli build @BuildArgs
    return
  }

  & bun tauri build @BuildArgs
}

New-Item -ItemType Directory -Force -Path $ReleaseDir | Out-Null
Get-ChildItem $ReleaseDir -File -ErrorAction SilentlyContinue | Remove-Item -Force

if ($null -eq $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
  $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
}
if (-not $env:TAURI_SIGNING_PRIVATE_KEY -and -not [string]::IsNullOrWhiteSpace($SigningKeyPath)) {
  $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $SigningKeyPath -Raw).Trim()
}
$env:CI = "true"
if ((Test-Path $CargoBin) -and ($env:PATH -notlike "*$CargoBin*")) {
  $env:PATH = "$CargoBin;$env:PATH"
}

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
    if ($IsPro) {
      $proConfig = Join-Path $Root "src-tauri\tauri.pro.conf.json"
      if (-not (Test-Path $proConfig)) {
        throw "Missing Pro Tauri overlay: $proConfig"
      }
      $buildArgs += @("--features", "pro", "--config", $proConfig)
      Write-Host "Building Atmospeak Pro channel (cargo feature pro + tauri.pro.conf.json)"
    }
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

$ProductSlug = if ($IsPro) { "atmospeak-pro" } else { "atmospeak" }
$NsisName = "$ProductSlug`_$Version`_x64-setup.exe"
$MsiName = "$ProductSlug`_$Version`_x64_en-US.msi"
$UpdaterZipName = "$ProductSlug`_$Version`_x64-setup.nsis.zip"
$PortableName = "$ProductSlug`_$Version`_x64-portable.zip"
$LatestName = "latest.json"
$ChecksumsName = "SHA256SUMS.txt"

$NsisDest = Join-Path $ReleaseDir $NsisName
$MsiDest = Join-Path $ReleaseDir $MsiName
$UpdaterZipDest = Join-Path $ReleaseDir $UpdaterZipName
$PortableDest = Join-Path $ReleaseDir $PortableName
$LatestDest = Join-Path $ReleaseDir $LatestName
$ChecksumsDest = Join-Path $ReleaseDir $ChecksumsName

Copy-Item $NsisSource.FullName $NsisDest -Force
Copy-Item $MsiSource.FullName $MsiDest -Force

$UpdaterAssetName = $NsisName
$UpdaterSignaturePath = "$NsisDest.sig"

if ($HasUpdaterSigningKey) {
  $NsisSigSource = "$($NsisSource.FullName).sig"
  if (Test-Path $NsisSigSource) {
    Copy-Item $NsisSigSource $UpdaterSignaturePath -Force
  } else {
    $UpdaterSource = Get-ChildItem (Join-Path $BundleRoot "nsis") -Filter "*.nsis.zip" -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    $UpdaterAssetName = $UpdaterZipName
    $UpdaterSignaturePath = "$UpdaterZipDest.sig"
    if ($UpdaterSource) {
      Copy-Item $UpdaterSource.FullName $UpdaterZipDest -Force
      $UpdaterSigSource = "$($UpdaterSource.FullName).sig"
      if (-not (Test-Path $UpdaterSigSource)) {
        throw "Updater artifact was produced without a signature: $UpdaterSigSource"
      }
      Copy-Item $UpdaterSigSource $UpdaterSignaturePath -Force
    } else {
      Compress-Archive -Path $NsisDest -DestinationPath $UpdaterZipDest -CompressionLevel Optimal
      Invoke-UpdaterSigner $UpdaterZipDest
      if ($LASTEXITCODE -ne 0) {
        throw "Failed to sign $UpdaterZipDest"
      }
    }
  }
}

$MsiSigSource = "$($MsiSource.FullName).sig"
if (Test-Path $MsiSigSource) {
  Copy-Item $MsiSigSource "$MsiDest.sig" -Force
}

if (Test-Path $StageDir) {
  Remove-Item $StageDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null

Copy-Item $ReleaseExe (Join-Path $StageDir "atmospeak.exe") -Force
Copy-Item $SidecarExe (Join-Path $StageDir "whisper-cli.exe") -Force
Copy-Item $ResourcesDir (Join-Path $StageDir "resources") -Recurse -Force
if (Test-Path $PortableDest) {
  Remove-Item $PortableDest -Force
}
Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $PortableDest -CompressionLevel Optimal

if ($HasUpdaterSigningKey) {
  $UpdaterSignature = (Get-Content $UpdaterSignaturePath -Raw).Trim()
  # Free channel: public CDN URLs embedded in latest.json.
  # Pro channel: placeholder CDN-relative names; the gated update Worker rewrites
  # `url` to short-lived pre-signed object URLs after licence checks.
  $UpdaterUrl = if ($IsPro) {
    "https://updates.novpax.org/atmospeak/pro/artifacts/$UpdaterAssetName"
  } else {
    "$FreeCdnBase/$UpdaterAssetName"
  }
  $ChannelNotes = if ($IsPro) {
    "Atmospeak Pro $Version — licensed build with gated updates."
  } else {
    "Atmospeak $Version desktop release with bundled offline transcription runtime."
  }
  $Latest = [ordered]@{
    version = $Version
    notes = $ChannelNotes
    pub_date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = [ordered]@{
      "windows-x86_64" = [ordered]@{
        signature = $UpdaterSignature
        url = $UpdaterUrl
      }
    }
  }
  $LatestJson = $Latest | ConvertTo-Json -Depth 8
  [System.IO.File]::WriteAllText($LatestDest, "$LatestJson`n", [System.Text.UTF8Encoding]::new($false))
}

$ReleaseFiles = Get-ChildItem $ReleaseDir -File | Where-Object { $_.Name -ne $ChecksumsName } | Sort-Object Name
$HashLines = foreach ($file in $ReleaseFiles) {
  "$(Get-Sha256Hex $file.FullName)  $($file.Name)"
}
$HashLines | Set-Content -Encoding ascii $ChecksumsDest

Remove-Item $StageDir -Recurse -Force

Write-Host "Release files written to $ReleaseDir (channel=$Channel)"
Get-ChildItem $ReleaseDir -File | Sort-Object Name | ForEach-Object {
  Write-Host ("{0} {1:N0} bytes" -f $_.Name, $_.Length)
}
if (-not $IsPro) {
  Write-Host "Upload free artifacts to $FreeCdnBase/ (and optionally mirror to GitHub Releases)."
} else {
  Write-Host "Upload Pro artifacts to the private Pro bucket; refresh Polar File Download; publish latest.json via the gated Worker."
}
