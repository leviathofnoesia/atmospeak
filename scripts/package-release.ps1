param(
  [string]$ReleaseRepo = $env:WIND_SPEAK_RELEASE_REPO,
  [switch]$SkipTauriBuild
)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($ReleaseRepo)) {
  $ReleaseRepo = "leviathofnoesia/wind-speak"
}

$Package = Get-Content (Join-Path $Root "package.json") -Raw | ConvertFrom-Json
$Version = [string]$Package.version
$ReleaseDir = Join-Path $Root "release"
$StageDir = Join-Path $ReleaseDir "portable-stage"
$BundleRoot = Join-Path $Root "src-tauri\target\release\bundle"
$ReleaseExe = Join-Path $Root "src-tauri\target\release\wind-speak.exe"
$SidecarExe = Join-Path $Root "src-tauri\target\release\whisper-cli.exe"
$ResourcesDir = Join-Path $Root "src-tauri\target\release\resources"
$DefaultKey = Join-Path $env:USERPROFILE ".tauri\wind-speak\updater.key"
$CargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
$TauriCli = Join-Path $Root "node_modules\.bin\tauri.exe"
$SigningKeyPath = $env:TAURI_SIGNING_PRIVATE_KEY_PATH
if ([string]::IsNullOrWhiteSpace($SigningKeyPath) -and (Test-Path $DefaultKey)) {
  $SigningKeyPath = $DefaultKey
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

New-Item -ItemType Directory -Force -Path $ReleaseDir | Out-Null

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
    bun run tauri build
    if ($LASTEXITCODE -ne 0) {
      throw "Tauri build failed with exit code $LASTEXITCODE"
    }
  } finally {
    Pop-Location
  }
}

if (-not (Test-Path $BundleRoot)) {
  throw "Missing bundle output: $BundleRoot"
}

$NsisSource = Get-ChildItem (Join-Path $BundleRoot "nsis") -Filter "*.exe" -File | Sort-Object LastWriteTime -Descending | Select-Object -First 1
$MsiSource = Get-ChildItem (Join-Path $BundleRoot "msi") -Filter "*.msi" -File | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $NsisSource) { throw "NSIS installer was not produced." }
if (-not $MsiSource) { throw "MSI installer was not produced." }

$NsisName = "Wind-Speak_$Version`_x64-setup.exe"
$MsiName = "Wind-Speak_$Version`_x64_en-US.msi"
$UpdaterName = "Wind-Speak_$Version`_x64-setup.nsis.zip"
$PortableName = "Wind-Speak_$Version`_x64-portable.zip"
$LatestName = "latest.json"
$ChecksumsName = "SHA256SUMS.txt"

$NsisDest = Join-Path $ReleaseDir $NsisName
$MsiDest = Join-Path $ReleaseDir $MsiName
$UpdaterDest = Join-Path $ReleaseDir $UpdaterName
$PortableDest = Join-Path $ReleaseDir $PortableName
$LatestDest = Join-Path $ReleaseDir $LatestName
$ChecksumsDest = Join-Path $ReleaseDir $ChecksumsName

Copy-Item $NsisSource.FullName $NsisDest -Force
Copy-Item $MsiSource.FullName $MsiDest -Force

$UpdaterSource = Get-ChildItem (Join-Path $BundleRoot "nsis") -Filter "*.nsis.zip" -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if ($UpdaterSource) {
  Copy-Item $UpdaterSource.FullName $UpdaterDest -Force
  $UpdaterSigSource = "$($UpdaterSource.FullName).sig"
  if (-not (Test-Path $UpdaterSigSource)) {
    throw "Updater artifact was produced without a signature: $UpdaterSigSource"
  }
  Copy-Item $UpdaterSigSource "$UpdaterDest.sig" -Force
} else {
  if (Test-Path $UpdaterDest) {
    Remove-Item $UpdaterDest -Force
  }
  if (Test-Path "$UpdaterDest.sig") {
    Remove-Item "$UpdaterDest.sig" -Force
  }
  Compress-Archive -Path $NsisDest -DestinationPath $UpdaterDest -CompressionLevel Optimal
  Invoke-UpdaterSigner $UpdaterDest
  if ($LASTEXITCODE -ne 0) {
    throw "Failed to sign $UpdaterDest"
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

Copy-Item $ReleaseExe (Join-Path $StageDir "wind-speak.exe") -Force
Copy-Item $SidecarExe (Join-Path $StageDir "whisper-cli.exe") -Force
Copy-Item $ResourcesDir (Join-Path $StageDir "resources") -Recurse -Force
if (Test-Path $PortableDest) {
  Remove-Item $PortableDest -Force
}
Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $PortableDest -CompressionLevel Optimal

$UpdaterSignature = (Get-Content "$UpdaterDest.sig" -Raw).Trim()
$Latest = [ordered]@{
  version = $Version
  notes = "Wind Speak $Version desktop release with bundled offline transcription runtime."
  pub_date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  platforms = [ordered]@{
    "windows-x86_64" = [ordered]@{
      signature = $UpdaterSignature
      url = "https://github.com/$ReleaseRepo/releases/latest/download/$UpdaterName"
    }
  }
}
$LatestJson = $Latest | ConvertTo-Json -Depth 8
[System.IO.File]::WriteAllText($LatestDest, "$LatestJson`n", [System.Text.UTF8Encoding]::new($false))

$ReleaseFiles = Get-ChildItem $ReleaseDir -File | Where-Object { $_.Name -ne $ChecksumsName } | Sort-Object Name
$HashLines = foreach ($file in $ReleaseFiles) {
  "$(Get-Sha256Hex $file.FullName)  $($file.Name)"
}
$HashLines | Set-Content -Encoding ascii $ChecksumsDest

Remove-Item $StageDir -Recurse -Force

Write-Host "Release files written to $ReleaseDir"
Get-ChildItem $ReleaseDir -File | Sort-Object Name | ForEach-Object {
  Write-Host ("{0} {1:N0} bytes" -f $_.Name, $_.Length)
}
