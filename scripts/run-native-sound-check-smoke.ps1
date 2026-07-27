param(
  [string]$AppExe = "",
  [string]$Device = "Elgato Wave:3"
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

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

$listener = [System.Net.Sockets.TcpListener]::new(
  [System.Net.IPAddress]::Loopback,
  0
)
$listener.Start()
$Port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
$listener.Stop()
$ProfileDir = Join-Path ([System.IO.Path]::GetTempPath()) (
  "atmospeak-native-sound-check-{0}" -f [Guid]::NewGuid().ToString("N")
)

$PreviousProfile = $env:ATMOSPEAK_APP_DATA_DIR
$PreviousWebViewArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
$PreviousDebugPort = $env:ATMOSPEAK_WEBVIEW_DEBUG_PORT
$Process = $null
$DevServer = $null
try {
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
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$Port"
  $env:ATMOSPEAK_WEBVIEW_DEBUG_PORT = "$Port"
  $Process = Start-Process -FilePath $AppExe -PassThru
  Start-Sleep -Seconds 1
  if ($Process.HasExited) {
    throw "Atmospeak exited before WebView2 opened (code $($Process.ExitCode))."
  }

  & node (Join-Path $PSScriptRoot "native-sound-check-harness.mjs") `
    "--port=$Port" `
    "--device=$Device"
  if ($LASTEXITCODE -ne 0) {
    Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" -ErrorAction SilentlyContinue |
      Where-Object { $_.CommandLine -match "atmospeak" -or $_.ParentProcessId -eq $Process.Id } |
      Select-Object ProcessId, ParentProcessId, CommandLine |
      Format-List |
      Out-Host
    throw "Native sound-check harness failed with code $LASTEXITCODE."
  }
}
finally {
  if ($Process -and -not $Process.HasExited) {
    Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
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
  if ($null -eq $PreviousWebViewArguments) {
    Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
  }
  else {
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $PreviousWebViewArguments
  }
  if ($null -eq $PreviousDebugPort) {
    Remove-Item Env:ATMOSPEAK_WEBVIEW_DEBUG_PORT -ErrorAction SilentlyContinue
  }
  else {
    $env:ATMOSPEAK_WEBVIEW_DEBUG_PORT = $PreviousDebugPort
  }
  if (Test-Path -LiteralPath $ProfileDir) {
    Remove-Item -LiteralPath $ProfileDir -Recurse -Force
  }
}
