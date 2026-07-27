param(
  [string]$InstallerPath = "",
  [string]$InstallDir = (Join-Path $env:TEMP "AtmospeakInstallTest")
)

$ErrorActionPreference = "Stop"

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class AtmospeakWindowProbe {
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
  $callback = [AtmospeakWindowProbe+EnumWindowsProc]{
    param([IntPtr]$Handle, [IntPtr]$Param)

    if (-not [AtmospeakWindowProbe]::IsWindowVisible($Handle)) {
      return $true
    }

    [uint32]$windowProcessId = 0
    [void][AtmospeakWindowProbe]::GetWindowThreadProcessId($Handle, [ref]$windowProcessId)
    if ($windowProcessId -ne $ProcessId) {
      return $true
    }

    $builder = New-Object System.Text.StringBuilder 256
    [void][AtmospeakWindowProbe]::GetWindowText($Handle, $builder, $builder.Capacity)
    $title = $builder.ToString()
    if (-not [string]::IsNullOrWhiteSpace($title)) {
      $titles.Add($title)
    }

    return $true
  }

  [void][AtmospeakWindowProbe]::EnumWindows($callback, [IntPtr]::Zero)
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

function Stop-ProcessesUnderInstallDir {
  param(
    [string]$Directory,
    [int]$TimeoutSeconds = 10
  )

  $resolvedDirectory = [System.IO.Path]::GetFullPath($Directory).TrimEnd('\') + '\'
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  do {
    $installedProcesses = @(
      Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
        Where-Object {
          $_.ExecutablePath -and
          [System.IO.Path]::GetFullPath($_.ExecutablePath).StartsWith(
            $resolvedDirectory,
            [System.StringComparison]::OrdinalIgnoreCase
          )
        }
    )
    if ($installedProcesses.Count -eq 0) {
      return
    }
    foreach ($installedProcess in $installedProcesses) {
      Stop-Process -Id $installedProcess.ProcessId -Force -ErrorAction SilentlyContinue
    }
    Start-Sleep -Milliseconds 200
  } while ((Get-Date) -lt $deadline)

  $remaining = @(
    Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
      Where-Object {
        $_.ExecutablePath -and
        [System.IO.Path]::GetFullPath($_.ExecutablePath).StartsWith(
          $resolvedDirectory,
          [System.StringComparison]::OrdinalIgnoreCase
        )
      }
  )
  throw "Installed processes did not exit: $($remaining.ProcessId -join ', ')"
}

function Test-PathUnderDirectory {
  param([string]$Path, [string]$Directory)
  if ([string]::IsNullOrWhiteSpace($Path)) {
    return $false
  }
  $resolvedDirectory = [System.IO.Path]::GetFullPath($Directory).TrimEnd('\') + '\'
  $resolvedPath = [System.IO.Path]::GetFullPath($Path.Trim('"'))
  return $resolvedPath.StartsWith(
    $resolvedDirectory,
    [System.StringComparison]::OrdinalIgnoreCase
  )
}

function Remove-TestInstallShortcuts {
  param([string]$Directory)
  $shortcutRoots = @(
    (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"),
    (Join-Path $env:USERPROFILE "Desktop")
  )
  $shell = New-Object -ComObject WScript.Shell
  foreach ($root in $shortcutRoots) {
    Get-ChildItem -LiteralPath $root -Filter "Atmospeak.lnk" -File -Recurse -ErrorAction SilentlyContinue |
      ForEach-Object {
        $target = $shell.CreateShortcut($_.FullName).TargetPath
        if (Test-PathUnderDirectory -Path $target -Directory $Directory) {
          Remove-Item -LiteralPath $_.FullName -Force
        }
      }
  }
}

function Remove-TestInstallRegistration {
  param([string]$Directory)
  $uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Atmospeak"
  if (Test-Path $uninstallKey) {
    $location = (Get-ItemProperty $uninstallKey -ErrorAction Stop).InstallLocation
    if (Test-PathUnderDirectory -Path $location -Directory $Directory) {
      Remove-Item -LiteralPath $uninstallKey -Recurse -Force
    }
  }
  $productKey = "HKCU:\Software\atmospeak\Atmospeak"
  if (Test-Path $productKey) {
    $location = (Get-Item $productKey).GetValue("")
    if (Test-PathUnderDirectory -Path $location -Directory $Directory) {
      Remove-Item -LiteralPath $productKey -Recurse -Force
    }
  }
}

function Assert-NoRealInstallRegistration {
  param([string]$Directory)
  $locations = @()
  $uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Atmospeak"
  if (Test-Path $uninstallKey) {
    $locations += (Get-ItemProperty $uninstallKey -ErrorAction Stop).InstallLocation
  }
  $productKey = "HKCU:\Software\atmospeak\Atmospeak"
  if (Test-Path $productKey) {
    $locations += (Get-Item $productKey).GetValue("")
  }
  foreach ($location in $locations) {
    if (
      -not [string]::IsNullOrWhiteSpace($location) -and
      -not (Test-PathUnderDirectory -Path $location -Directory $Directory)
    ) {
      throw "Refusing installer smoke while a real Atmospeak installation is registered at $location"
    }
  }
}

function Get-FreeTcpPort {
  $listener = [System.Net.Sockets.TcpListener]::new(
    [System.Net.IPAddress]::Loopback,
    0
  )
  $listener.Start()
  try {
    return ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
  }
  finally {
    $listener.Stop()
  }
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

$ProfileDir = "$InstallDir-profile"
Assert-NoRealInstallRegistration -Directory $InstallDir
Remove-TestInstallShortcuts -Directory $InstallDir
Remove-TestInstallRegistration -Directory $InstallDir

trap {
  try {
    Stop-ProcessesUnderInstallDir -Directory $InstallDir
  } catch {
    Write-Warning "Emergency installed-process cleanup failed: $($_.Exception.Message)"
  }
  try {
    Remove-TestInstallShortcuts -Directory $InstallDir
    Remove-TestInstallRegistration -Directory $InstallDir
  } catch {
    Write-Warning "Emergency installer shell cleanup failed: $($_.Exception.Message)"
  }
  if (Test-Path -LiteralPath $ProfileDir) {
    Remove-Item -LiteralPath $ProfileDir -Recurse -Force -ErrorAction SilentlyContinue
  }
  [Console]::Error.WriteLine($_.ToString())
  exit 1
}

if (Test-Path $InstallDir) {
  Stop-ProcessesUnderInstallDir -Directory $InstallDir
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

if (Test-Path $ProfileDir) {
  Remove-Item $ProfileDir -Recurse -Force
}
$PreviousProfileOverride = $env:ATMOSPEAK_APP_DATA_DIR
$PreviousWebViewArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
$PreviousDebugPort = $env:ATMOSPEAK_WEBVIEW_DEBUG_PORT
$DebugPort = Get-FreeTcpPort
$env:ATMOSPEAK_APP_DATA_DIR = $ProfileDir
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$DebugPort"
$env:ATMOSPEAK_WEBVIEW_DEBUG_PORT = "$DebugPort"
try {
  $process = Start-Process -FilePath $AppExe -PassThru
} finally {
  if ($null -eq $PreviousProfileOverride) {
    Remove-Item Env:ATMOSPEAK_APP_DATA_DIR -ErrorAction SilentlyContinue
  } else {
    $env:ATMOSPEAK_APP_DATA_DIR = $PreviousProfileOverride
  }
  if ($null -eq $PreviousWebViewArguments) {
    Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
  } else {
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $PreviousWebViewArguments
  }
  if ($null -eq $PreviousDebugPort) {
    Remove-Item Env:ATMOSPEAK_WEBVIEW_DEBUG_PORT -ErrorAction SilentlyContinue
  } else {
    $env:ATMOSPEAK_WEBVIEW_DEBUG_PORT = $PreviousDebugPort
  }
}
try {
  Wait-ForInstalledWindow -ProcessId $process.Id -Title "Atmospeak"
  $titles = Get-VisibleWindowTitlesForProcess -ProcessId $process.Id
  if ($titles -contains "Atmospeak Overlay") {
    throw "Fresh first launch incorrectly created the overlay before setup."
  }
  $Harness = Join-Path $Root "scripts\native-webview-harness.mjs"
  & node $Harness "--port=$DebugPort" "--expect=setup"
  if ($LASTEXITCODE -ne 0) {
    throw "Native WebView DOM harness failed with exit code $LASTEXITCODE."
  }
  if ($process.HasExited) {
    throw "App exited during launch smoke with code $($process.ExitCode)"
  }
}
finally {
  Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
  try {
    Stop-ProcessesUnderInstallDir -Directory $InstallDir
  }
  catch {
    Write-Warning "Installed-process cleanup failed: $($_.Exception.Message)"
  }
  Remove-TestInstallShortcuts -Directory $InstallDir
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
Stop-ProcessesUnderInstallDir -Directory $InstallDir
Remove-TestInstallShortcuts -Directory $InstallDir
Remove-TestInstallRegistration -Directory $InstallDir
if (Test-Path $ProfileDir) {
  Remove-Item $ProfileDir -Recurse -Force
}

Write-Host "Install/uninstall smoke passed."
