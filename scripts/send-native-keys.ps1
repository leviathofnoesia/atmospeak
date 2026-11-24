param(
  [Parameter(Mandatory = $true)]
  [ValidateSet("down", "up", "press")]
  [string]$Action,

  [Parameter(Mandatory = $true)]
  [string]$Keys,

  [int]$FocusProcessId = 0,

  [string]$FocusProcessName = "",

  [string]$FocusWindowTitle = "",

  [Int64]$FocusHwnd = 0,

  [Int64]$AtUtcMs = 0,

  [string]$EventLogPath = ""
)

$ErrorActionPreference = "Stop"

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class AtmospeakNativeKeys {
  [DllImport("kernel32.dll")]
  public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")]
  public static extern uint GetWindowThreadProcessId(IntPtr window, IntPtr process);
  [DllImport("user32.dll")]
  public static extern bool AttachThreadInput(uint from, uint to, bool attach);
  [StructLayout(LayoutKind.Sequential)]
  private struct KeyboardInput {
    public ushort key;
    public ushort scan;
    public uint flags;
    public uint time;
    public UIntPtr extra;
  }
  [StructLayout(LayoutKind.Sequential)]
  private struct MouseInput {
    public int x, y;
    public uint data, flags, time;
    public UIntPtr extra;
  }
  [StructLayout(LayoutKind.Explicit)]
  private struct InputUnion {
    [FieldOffset(0)] public KeyboardInput keyboard;
    [FieldOffset(0)] public MouseInput mouse;
  }
  [StructLayout(LayoutKind.Sequential)]
  private struct Input {
    public uint type;
    public InputUnion value;
  }
  public sealed class InputEdge {
    public long Ticks, UtcMs, ReturnedTicks;
    public uint Count;
  }
  [DllImport("user32.dll", SetLastError = true)]
  private static extern uint SendInput(uint count, Input[] inputs, int size);

  public static InputEdge SendChord(byte[] keys, bool release) {
    var inputs = new Input[keys.Length];
    for (int i = 0; i < keys.Length; i++) {
      inputs[i].type = 1;
      inputs[i].value.keyboard.key = keys[release ? keys.Length - i - 1 : i];
      inputs[i].value.keyboard.flags = release ? 2u : 0u;
    }
    int size = Marshal.SizeOf(typeof(Input));
    // Warm clock and P/Invoke setup with no input events before timing the edge.
    SendInput(0, new Input[0], size);
    System.Diagnostics.Stopwatch.GetTimestamp();
    var edge = new InputEdge();
    edge.UtcMs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
    edge.Ticks = System.Diagnostics.Stopwatch.GetTimestamp();
    edge.Count = SendInput((uint)inputs.Length, inputs, size);
    edge.ReturnedTicks = System.Diagnostics.Stopwatch.GetTimestamp();
    if (edge.Count != inputs.Length) {
      throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "Native chord input was not accepted completely");
    }
    return edge;
  }

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
if ($FocusHwnd -ne 0) {
  $focusWindow = [IntPtr]::new($FocusHwnd)
}
elseif (-not [string]::IsNullOrWhiteSpace($FocusWindowTitle)) {
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
  $currentThread = [AtmospeakNativeKeys]::GetCurrentThreadId()
  $foregroundThread = [AtmospeakNativeKeys]::GetWindowThreadProcessId([AtmospeakNativeKeys]::GetForegroundWindow(), [IntPtr]::Zero)
  $targetThread = [AtmospeakNativeKeys]::GetWindowThreadProcessId($focusWindow, [IntPtr]::Zero)
  $attached = @()
  try {
    foreach ($otherThread in @($foregroundThread, $targetThread) | Select-Object -Unique) {
      if ($otherThread -ne 0 -and $otherThread -ne $currentThread -and [AtmospeakNativeKeys]::AttachThreadInput($currentThread, $otherThread, $true)) { $attached += $otherThread }
    }
    for ($attempt = 0; $attempt -lt 8; $attempt++) {
      [AtmospeakNativeKeys]::ShowWindow($focusWindow, 9) | Out-Null
      [AtmospeakNativeKeys]::BringWindowToTop($focusWindow) | Out-Null
      [AtmospeakNativeKeys]::SetForegroundWindow($focusWindow) | Out-Null
      Start-Sleep -Milliseconds 20
      if ([AtmospeakNativeKeys]::GetForegroundWindow() -eq $focusWindow) { break }
    }
  } finally {
    foreach ($otherThread in $attached) { [AtmospeakNativeKeys]::AttachThreadInput($currentThread, $otherThread, $false) | Out-Null }
  }
  if ([AtmospeakNativeKeys]::GetForegroundWindow() -ne $focusWindow) {
    throw "Owned native target did not become foreground; refusing to send keys elsewhere."
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

if ($AtUtcMs -gt 0) {
  while ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() -lt $AtUtcMs) { Start-Sleep -Milliseconds 1 }
}
$edge = switch ($Action) {
  "down" { [AtmospeakNativeKeys]::SendChord([byte[]]$codes, $false) }
  "up" { [AtmospeakNativeKeys]::SendChord([byte[]]$codes, $true) }
  "press" {
    $pressed = [AtmospeakNativeKeys]::SendChord([byte[]]$codes, $false)
    Start-Sleep -Milliseconds 60
    $null = [AtmospeakNativeKeys]::SendChord([byte[]]$codes, $true)
    $pressed
  }
}
$edgeTicks = $edge.Ticks
$edgeUtcMs = $edge.UtcMs

if ($focusProcess) {
  Write-Output $focusProcess.Id
}

if ($EventLogPath) {
  $event = @{ kind = "key-$Action"; ticks = "$edgeTicks"; frequency = "$( [System.Diagnostics.Stopwatch]::Frequency )"; utcMs = $edgeUtcMs; requestedUtcMs = $AtUtcMs; keys = $Keys; timingVersion = 2; inputEventCount = $edge.Count; inputCallReturnedTicks = "$($edge.ReturnedTicks)" } | ConvertTo-Json -Compress
  [System.IO.File]::AppendAllText($EventLogPath, $event + "`n", [System.Text.UTF8Encoding]::new($false))
}
