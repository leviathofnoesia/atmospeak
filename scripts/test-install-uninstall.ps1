param(
  [string]$InstallerPath = "",
  [string]$InstallDir = (Join-Path $env:TEMP "AtmospeakInstallTest")
)

$ErrorActionPreference = "Stop"

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class WindSpeakWindowProbe {
  public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

  [DllImport("user32.dll")]
  public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);

  [DllImport("user32.dll")]
  public static extern bool IsWindowVisible(IntPtr hWnd);

  [DllImport("user32.dll")]
  public static extern int GetWindowText(IntPtr hWnd, StringBuilder lpString, int nMaxCount);

  [DllImport("user32.dll")]
  public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
}
"@

function Get-VisibleWindowTitlesForProcess {
  param([int]$ProcessId)

  $titles = New-Object System.Collections.Generic.List[string]
  $callback = [WindSpeakWindowProbe+EnumWindowsProc]{
    param([IntPtr]$Handle, [IntPtr]$Param)

    if (-not [WindSpeakWindowProbe]::IsWindowVisible($Handle)) {
      return $true
    }

    [uint32]$windowProcessId = 0
    [void][WindSpeakWindowProbe]::GetWindowThreadProcessId($Handle, [ref]$windowProcessId)
    if ($windowProcessId -ne $ProcessId) {
      return $true
    }

    $builder = New-Object System.Text.StringBuilder 256
    [void][WindSpeakWindowProbe]::GetWindowText($Handle, $builder, $builder.Capacity)
    $title = $builder.ToString()
    if (-not [string]::IsNullOrWhiteSpace($title)) {
      $titles.Add($title)
    }

    return $true
  }

  [void][WindSpeakWindowProbe]::EnumWindows($callback, [IntPtr]::Zero)
  return $titles.ToArray()
}

function Wait-ForInstalledWindow {
  param(
    [int]$ProcessId,
    [string]$Title,
    [int]$TimeoutSeconds = 15
  )

  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  do {
    $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if (-not $process) {
      throw "App process exited before '$Title' appeared."
    }

    $titles = Get-VisibleWindowTitlesForProcess -ProcessId $ProcessId
    if ($titles -contains $Title) {
      return
    }

    Start-Sleep -Milliseconds 250
  } while ((Get-Date) -lt $deadline)

  $observed = Get-VisibleWindowTitlesForProcess -ProcessId $ProcessId
  throw "Timed out waiting for visible '$Title' window. Observed: $($observed -join ', ')"
}

function Wait-ForPathRemoval {
  param(
    [string]$Path,
    [int]$TimeoutSeconds = 15
  )

  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  do {
    if (-not (Test-Path $Path)) {
      return
    }
    Start-Sleep -Milliseconds 250
  } while ((Get-Date) -lt $deadline)

  throw "Path still exists after waiting for uninstall cleanup: $Path"
}

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

$AppExe = Join-Path $InstallDir "Atmospeak.exe"
if (-not (Test-Path $AppExe)) {
  $AppExe = Join-Path $InstallDir "atmospeak.exe"
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
try {
  Wait-ForInstalledWindow -ProcessId $process.Id -Title "Atmospeak"
  Wait-ForInstalledWindow -ProcessId $process.Id -Title "Atmospeak Overlay"
  if ($process.HasExited) {
    throw "App exited during launch smoke with code $($process.ExitCode)"
  }
}
finally {
  Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
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

Wait-ForPathRemoval -Path $AppExe

Write-Host "Install/uninstall smoke passed."
