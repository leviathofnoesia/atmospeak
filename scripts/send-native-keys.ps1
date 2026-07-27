param(
  [Parameter(Mandatory = $true)]
  [ValidateSet("down", "up", "press")]
  [string]$Action,

  [Parameter(Mandatory = $true)]
  [string]$Keys,

  [int]$FocusProcessId = 0,

  [string]$FocusProcessName = "",

  [string]$FocusWindowTitle = ""
)

$ErrorActionPreference = "Stop"

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class AtmospeakNativeKeys {
  [DllImport("user32.dll", SetLastError = true)]
  public static extern void keybd_event(byte virtualKey, byte scanCode, uint flags, UIntPtr extraInfo);

  [DllImport("user32.dll")]
  [return: MarshalAs(UnmanagedType.Bool)]
  public static extern bool SetForegroundWindow(IntPtr window);

  [DllImport("user32.dll")]
  public static extern IntPtr GetForegroundWindow();

  [DllImport("user32.dll")]
  [return: MarshalAs(UnmanagedType.Bool)]
  public static extern bool ShowWindow(IntPtr window, int command);

  [DllImport("user32.dll")]
  [return: MarshalAs(UnmanagedType.Bool)]
  public static extern bool BringWindowToTop(IntPtr window);

  [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern IntPtr FindWindow(string className, string windowName);
}
"@

$virtualKeys = @{
  "Ctrl" = 0x11
  "Alt" = 0x12
  "Shift" = 0x10
  "Win" = 0x5B
  "Space" = 0x20
  "Enter" = 0x0D
  "Tab" = 0x09
  "Escape" = 0x1B
  "CapsLock" = 0x14
  "Backspace" = 0x08
  "Delete" = 0x2E
  "Insert" = 0x2D
  "Home" = 0x24
  "End" = 0x23
  "PageUp" = 0x21
  "PageDown" = 0x22
  "Left" = 0x25
  "Up" = 0x26
  "Right" = 0x27
  "Down" = 0x28
}

for ($index = 1; $index -le 24; $index++) {
  $virtualKeys["F$index"] = 0x6F + $index
}
foreach ($letter in [char[]]"ABCDEFGHIJKLMNOPQRSTUVWXYZ") {
  $virtualKeys["$letter"] = [int][char]$letter
}
foreach ($digit in 0..9) {
  $virtualKeys["$digit"] = 0x30 + $digit
}

$focusProcess = $null
$focusWindow = [IntPtr]::Zero
if (-not [string]::IsNullOrWhiteSpace($FocusWindowTitle)) {
  $deadline = (Get-Date).AddSeconds(10)
  do {
    $focusWindow = [AtmospeakNativeKeys]::FindWindow($null, $FocusWindowTitle)
    if ($focusWindow -eq [IntPtr]::Zero) {
      Start-Sleep -Milliseconds 100
    }
  } while ($focusWindow -eq [IntPtr]::Zero -and (Get-Date) -lt $deadline)
  if ($focusWindow -eq [IntPtr]::Zero) {
    throw "No window was found with title '$FocusWindowTitle'."
  }
}
elseif ($FocusProcessId -gt 0) {
  $focusProcess = Get-Process -Id $FocusProcessId -ErrorAction Stop
}
elseif (-not [string]::IsNullOrWhiteSpace($FocusProcessName)) {
  $deadline = (Get-Date).AddSeconds(10)
  do {
    $focusProcess = Get-Process -Name $FocusProcessName -ErrorAction SilentlyContinue |
      Where-Object { $_.MainWindowHandle -ne 0 } |
      Sort-Object StartTime -Descending |
      Select-Object -First 1
    if (-not $focusProcess) {
      Start-Sleep -Milliseconds 100
    }
  } while (-not $focusProcess -and (Get-Date) -lt $deadline)
  if (-not $focusProcess) {
    throw "No $FocusProcessName process exposed a main window."
  }
}

if ($focusProcess) {
  $deadline = (Get-Date).AddSeconds(10)
  while ($focusProcess.MainWindowHandle -eq 0 -and (Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 100
    $focusProcess.Refresh()
  }
  if ($focusProcess.MainWindowHandle -eq 0) {
    throw "Process $($focusProcess.Id) did not expose a main window."
  }
  $focusWindow = $focusProcess.MainWindowHandle
}

if ($focusWindow -ne [IntPtr]::Zero) {
  [AtmospeakNativeKeys]::ShowWindow($focusWindow, 9) | Out-Null
  # Windows allows a process that just received user input to transfer the
  # foreground. A harmless Alt tap gives this short-lived harness process that
  # right without changing the target document.
  [AtmospeakNativeKeys]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
  [AtmospeakNativeKeys]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
  [AtmospeakNativeKeys]::BringWindowToTop($focusWindow) | Out-Null
  [AtmospeakNativeKeys]::SetForegroundWindow($focusWindow) | Out-Null
  Start-Sleep -Milliseconds 150
  if ([AtmospeakNativeKeys]::GetForegroundWindow() -ne $focusWindow) {
    throw "Could not focus the requested native window."
  }
}

$keyNames = @($Keys.Split("+", [System.StringSplitOptions]::RemoveEmptyEntries))
if ($keyNames.Count -eq 0) {
  throw "At least one key is required."
}
$codes = foreach ($keyName in $keyNames) {
  $trimmed = $keyName.Trim()
  if (-not $virtualKeys.ContainsKey($trimmed)) {
    throw "Unsupported native harness key: $trimmed"
  }
  [byte]$virtualKeys[$trimmed]
}

function Send-KeyDown([byte]$Code) {
  [AtmospeakNativeKeys]::keybd_event($Code, 0, 0, [UIntPtr]::Zero)
}

function Send-KeyUp([byte]$Code) {
  [AtmospeakNativeKeys]::keybd_event($Code, 0, 2, [UIntPtr]::Zero)
}

switch ($Action) {
  "down" {
    foreach ($code in $codes) {
      Send-KeyDown $code
    }
  }
  "up" {
    [array]::Reverse($codes)
    foreach ($code in $codes) {
      Send-KeyUp $code
    }
  }
  "press" {
    foreach ($code in $codes) {
      Send-KeyDown $code
    }
    Start-Sleep -Milliseconds 60
    [array]::Reverse($codes)
    foreach ($code in $codes) {
      Send-KeyUp $code
    }
  }
}

if ($focusProcess) {
  Write-Output $focusProcess.Id
}
