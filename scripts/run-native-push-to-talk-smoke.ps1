param(
  [string]$AppExe = "",
  [string]$FixturePath = "",
  [string]$Hotkey = "Ctrl+Alt+F12"
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$GeneratedFixture = $false

function Stop-ProcessTree {
  param([int]$RootProcessId)
  $children = @(
    Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
      Where-Object { $_.ParentProcessId -eq $RootProcessId }
  )
  foreach ($child in $children) {
    Stop-ProcessTree -RootProcessId $child.ProcessId
  }
  Stop-Process -Id $RootProcessId -Force -ErrorAction SilentlyContinue
}

if ([string]::IsNullOrWhiteSpace($AppExe)) {
  $AppExe = Join-Path $Root "src-tauri\target\debug\atmospeak.exe"
}
if (-not (Test-Path -LiteralPath $AppExe)) {
  throw "Atmospeak debug executable not found: $AppExe"
}

if ([string]::IsNullOrWhiteSpace($FixturePath)) {
  $FixturePath = Join-Path $env:TEMP "atmospeak-ptt-fixture.wav"
  $GeneratedFixture = $true
  Add-Type -AssemblyName System.Speech
  $voice = New-Object System.Speech.Synthesis.SpeechSynthesizer
  try {
    $voice.Rate = -1
    $voice.Volume = 60
    $voice.SetOutputToWaveFile($FixturePath)
    $voice.Speak("The porcelain moon hums over the studio.")
  }
  finally {
    $voice.Dispose()
  }
}
if (-not (Test-Path -LiteralPath $FixturePath)) {
  throw "Push-to-talk audio fixture not found: $FixturePath"
}

$listener = [System.Net.Sockets.TcpListener]::new(
  [System.Net.IPAddress]::Loopback,
  0
)
$listener.Start()
$Port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
$listener.Stop()
$ProfileDir = Join-Path ([System.IO.Path]::GetTempPath()) (
  "atmospeak-native-ptt-{0}" -f [Guid]::NewGuid().ToString("N")
)

$PreviousProfile = $env:ATMOSPEAK_APP_DATA_DIR
$PreviousDebugPort = $env:ATMOSPEAK_WEBVIEW_DEBUG_PORT
$PreviousHarness = $env:ATMOSPEAK_NATIVE_HARNESS
$PreviousFixture = $env:ATMOSPEAK_TEST_AUDIO_FIXTURE
$PreviousAsrBackend = $env:ATMOSPEAK_ASR_BACKEND
$Process = $null
$DevServer = $null
try {
  Get-Process atmospeak -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -ne $AppExe } |
    Stop-Process -Force -ErrorAction SilentlyContinue

  $Bun = Get-Command bun -ErrorAction Stop
  $DevServer = Start-Process -FilePath $Bun.Source `
    -WorkingDirectory $Root `
    -ArgumentList @("run", "dev", "--", "--host", "127.0.0.1") `
    -WindowStyle Hidden `
    -PassThru
  $devDeadline = (Get-Date).AddSeconds(20)
  do {
    try {
      Invoke-WebRequest "http://127.0.0.1:1420/" -TimeoutSec 1 -UseBasicParsing |
        Out-Null
      break
    }
    catch {
      if ($DevServer.HasExited) {
        throw "Vite exited before the native harness could start."
      }
      Start-Sleep -Milliseconds 200
    }
  } while ((Get-Date) -lt $devDeadline)
  if ((Get-Date) -ge $devDeadline) {
    throw "Vite did not become ready within 20 seconds."
  }

  $env:ATMOSPEAK_APP_DATA_DIR = $ProfileDir
  $env:ATMOSPEAK_WEBVIEW_DEBUG_PORT = "$Port"
  $env:ATMOSPEAK_NATIVE_HARNESS = "1"
  # Latency gate prefers the warm Vulkan sidecar (release→paste ≤500ms on this
  # fixture). Override with ATMOSPEAK_ASR_BACKEND=cpu to exercise the CPU path.
  # Rebuild sidecars before relying on this gate after host/session changes:
  #   powershell -File scripts/build-asr-sidecars.ps1
  # Set ATMOSPEAK_ASR_REBUILD=1 to rebuild Vulkan/CPU hosts before the smoke.
  if ($env:ATMOSPEAK_ASR_REBUILD -eq "1") {
    Write-Host "ATMOSPEAK_ASR_REBUILD=1 — rebuilding ASR sidecars…"
    & powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "build-asr-sidecars.ps1")
    if ($LASTEXITCODE -ne 0) {
      throw "ASR sidecar rebuild failed with code $LASTEXITCODE."
    }
  }
  $VulkanSidecar = Join-Path $Root "src-tauri\resources\asr\atmospeak-asr-vulkan.exe"
  if (-not (Test-Path -LiteralPath $VulkanSidecar)) {
    throw "Vulkan ASR sidecar missing at $VulkanSidecar — run scripts/build-asr-sidecars.ps1"
  }
  $env:ATMOSPEAK_ASR_BACKEND = if ($env:ATMOSPEAK_ASR_BACKEND) {
    $env:ATMOSPEAK_ASR_BACKEND
  } else {
    "vulkan"
  }
  $env:ATMOSPEAK_TEST_AUDIO_FIXTURE = (Resolve-Path -LiteralPath $FixturePath).Path
  $Process = Start-Process -FilePath $AppExe -PassThru
  Start-Sleep -Seconds 1
  if ($Process.HasExited) {
    throw "Atmospeak exited before WebView2 opened (code $($Process.ExitCode))."
  }

  & node (Join-Path $PSScriptRoot "native-push-to-talk-harness.mjs") "--port=$Port" "--hotkey=$Hotkey"
  if ($LASTEXITCODE -ne 0) {
    throw "Native push-to-talk harness failed with code $LASTEXITCODE."
  }
}
finally {
  if ($Process -and -not $Process.HasExited) {
    Stop-ProcessTree -RootProcessId $Process.Id
    Wait-Process -Id $Process.Id -Timeout 10 -ErrorAction SilentlyContinue
  }
  if ($DevServer -and -not $DevServer.HasExited) {
    Stop-ProcessTree -RootProcessId $DevServer.Id
    Wait-Process -Id $DevServer.Id -Timeout 10 -ErrorAction SilentlyContinue
  }
  if ($null -eq $PreviousProfile) {
    Remove-Item Env:ATMOSPEAK_APP_DATA_DIR -ErrorAction SilentlyContinue
  }
  else {
    $env:ATMOSPEAK_APP_DATA_DIR = $PreviousProfile
  }
  if ($null -eq $PreviousDebugPort) {
    Remove-Item Env:ATMOSPEAK_WEBVIEW_DEBUG_PORT -ErrorAction SilentlyContinue
  }
  else {
    $env:ATMOSPEAK_WEBVIEW_DEBUG_PORT = $PreviousDebugPort
  }
  if ($null -eq $PreviousHarness) {
    Remove-Item Env:ATMOSPEAK_NATIVE_HARNESS -ErrorAction SilentlyContinue
  }
  else {
    $env:ATMOSPEAK_NATIVE_HARNESS = $PreviousHarness
  }
  if ($null -eq $PreviousFixture) {
    Remove-Item Env:ATMOSPEAK_TEST_AUDIO_FIXTURE -ErrorAction SilentlyContinue
  }
  else {
    $env:ATMOSPEAK_TEST_AUDIO_FIXTURE = $PreviousFixture
  }
  if ($null -eq $PreviousAsrBackend) {
    Remove-Item Env:ATMOSPEAK_ASR_BACKEND -ErrorAction SilentlyContinue
  }
  else {
    $env:ATMOSPEAK_ASR_BACKEND = $PreviousAsrBackend
  }
  if (Test-Path -LiteralPath $ProfileDir) {
    Remove-Item -LiteralPath $ProfileDir -Recurse -Force
  }
  if ($GeneratedFixture -and (Test-Path -LiteralPath $FixturePath)) {
    Remove-Item -LiteralPath $FixturePath -Force
  }
}
