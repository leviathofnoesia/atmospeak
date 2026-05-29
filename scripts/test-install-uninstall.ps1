param(
  [string]$InstallerPath = "",
  [string]$InstallDir = (Join-Path $env:TEMP "WindSpeakInstallTest")
)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($InstallerPath)) {
  $InstallerPath = Get-ChildItem (Join-Path $Root "release") -Filter "*_x64-setup.exe" -File |
    Sort-Object LastWriteTime -Descending |
    Select-Object -ExpandProperty FullName -First 1
}

if (-not $InstallerPath -or -not (Test-Path $InstallerPath)) {
  throw "Installer not found. Run scripts/package-release.ps1 first."
}

if (Test-Path $InstallDir) {
  Remove-Item $InstallDir -Recurse -Force
}

Write-Host "Installing $InstallerPath to $InstallDir"
$install = Start-Process -FilePath $InstallerPath -ArgumentList @("/S", "/D=$InstallDir") -Wait -PassThru
if ($install.ExitCode -ne 0) {
  throw "Installer exited with code $($install.ExitCode)"
}

$AppExe = Join-Path $InstallDir "Wind Speak.exe"
if (-not (Test-Path $AppExe)) {
  $AppExe = Join-Path $InstallDir "wind-speak.exe"
}
if (-not (Test-Path $AppExe)) {
  throw "Installed app executable not found under $InstallDir"
}
if (-not (Test-Path (Join-Path $InstallDir "resources\models\ggml-base.en.bin"))) {
  throw "Bundled model not found in installed resources."
}
if (-not (Test-Path (Join-Path $InstallDir "resources\whisper-runtime\whisper-cli.exe"))) {
  throw "Bundled whisper runtime not found in installed resources."
}

$process = Start-Process -FilePath $AppExe -PassThru
Start-Sleep -Seconds 5
if (-not $process.HasExited) {
  Stop-Process -Id $process.Id -Force
}

$Uninstaller = Get-ChildItem $InstallDir -Filter "*uninst*.exe" -File -ErrorAction SilentlyContinue |
  Select-Object -ExpandProperty FullName -First 1
if (-not $Uninstaller) {
  $Uninstaller = Join-Path $InstallDir "uninstall.exe"
}
if (-not (Test-Path $Uninstaller)) {
  throw "Uninstaller was not found under $InstallDir"
}

Write-Host "Uninstalling with $Uninstaller"
$uninstall = Start-Process -FilePath $Uninstaller -ArgumentList "/S" -Wait -PassThru
if ($uninstall.ExitCode -ne 0) {
  throw "Uninstaller exited with code $($uninstall.ExitCode)"
}

Start-Sleep -Seconds 2
if (Test-Path $AppExe) {
  throw "App executable still exists after uninstall: $AppExe"
}

Write-Host "Install/uninstall smoke passed."
