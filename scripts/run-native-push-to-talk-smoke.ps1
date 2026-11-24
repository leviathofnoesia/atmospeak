param(
  [string]$AppExe = "",
  [string]$FixturePath = "",
  [string]$Hotkey = "Ctrl+Alt+F12",
  [string]$RunRoot = "",
  [int]$DevServerPort = 1420
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$DefaultRunRoot = "E:\BuildCache\atmospeak-smoke\run-data"
$GeneratedFixture = $false

function Assert-EPath {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string]$Label,
    [switch]$MustExist
  )
  try {
    $fullPath = [System.IO.Path]::GetFullPath($Path)
  }
  catch {
    throw "$Label is not a valid path: $Path"
  }
  if ($fullPath -notmatch "^[Ee]:\\") {
    throw "$Label must be on E: for an isolated verification run: $fullPath"
  }
  if ($MustExist -and -not (Test-Path -LiteralPath $fullPath -PathType Leaf)) {
    throw "$Label does not exist: $fullPath"
  }
  return $fullPath
}

function Test-ListeningPort {
  param([int]$Port)
  try {
    $listeners = @(Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction Stop)
    if ($listeners.Count -gt 0) {
      return $true
    }
  }
  catch {
    # Fall through to a loopback connect when the TCP cmdlet is unavailable.
  }

  $client = New-Object System.Net.Sockets.TcpClient
  try {
    $pending = $client.BeginConnect("127.0.0.1", $Port, $null, $null)
    if (-not $pending.AsyncWaitHandle.WaitOne(300)) {
      return $false
    }
    $client.EndConnect($pending)
    return $true
  }
  catch {
    return $false
  }
  finally {
    $client.Dispose()
  }
}

function Get-Node24Path {
  $candidate = $env:ATMOSPEAK_NODE_EXE
  if ([string]::IsNullOrWhiteSpace($candidate)) {
    $candidate = Join-Path $env:USERPROFILE ".cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin\node.exe"
  }
  if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
    $node = Get-Command node -ErrorAction SilentlyContinue
    if ($node) {
      $candidate = $node.Source
    }
  }
  if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
    throw "Node 24 executable not found. Set ATMOSPEAK_NODE_EXE to the bundled Node 24 path."
  }
  $version = (& $candidate --version 2>$null).Trim()
  if ($LASTEXITCODE -ne 0 -or $version -notmatch "^v24\.") {
    throw "Native harness requires Node 24; resolved '$candidate' reported '$version'."
  }
  return (Resolve-Path -LiteralPath $candidate).Path
}

function Stop-OwnedProcessTree {
  param([int[]]$RootProcessIds)

  $processes = @()
  try {
    $processes = @(Get-CimInstance Win32_Process -ErrorAction Stop | Select-Object ProcessId, ParentProcessId)
  }
  catch {
    Write-Warning "Could not enumerate child processes; stopping only the verification roots."
  }

  $stopIds = [System.Collections.Generic.List[int]]::new()
  $pending = [System.Collections.Generic.Queue[int]]::new()
  foreach ($rootProcessId in $RootProcessIds) {
    if ($rootProcessId -gt 0 -and $OwnedProcessIds.Contains($rootProcessId)) {
      [void]$pending.Enqueue($rootProcessId)
    }
  }
  while ($pending.Count -gt 0) {
    $processId = $pending.Dequeue()
    [void]$stopIds.Add($processId)
    foreach ($child in ($processes | Where-Object { $_.ParentProcessId -eq $processId })) {
      $childId = [int]$child.ProcessId
      if ($OwnedProcessIds.Add($childId)) {
        [void]$pending.Enqueue($childId)
      }
    }
  }

  for ($index = $stopIds.Count - 1; $index -ge 0; $index -= 1) {
    Stop-Process -Id $stopIds[$index] -Force -ErrorAction SilentlyContinue
  }
}

$RunRoot = if ([string]::IsNullOrWhiteSpace($RunRoot)) {
  Join-Path $DefaultRunRoot ("ptt-{0}-{1}" -f (Get-Date -Format "yyyyMMdd-HHmmss"), [Guid]::NewGuid().ToString("N").Substring(0, 8))
}
else {
  $RunRoot
}
$RunRoot = Assert-EPath -Path $RunRoot -Label "RunRoot"
if ($DevServerPort -ne 1420) {
  throw "The native app devUrl is fixed at 1420; refusing to run with DevServerPort=$DevServerPort."
}

if ([string]::IsNullOrWhiteSpace($AppExe)) {
  $AppExe = Join-Path $Root "src-tauri\target\debug\atmospeak.exe"
}
$AppExe = Assert-EPath -Path $AppExe -Label "AppExe" -MustExist

$TempRoot = Join-Path $RunRoot "tmp"
$ProfileDir = Join-Path $RunRoot "profile"
$LogDir = Join-Path $RunRoot "logs"
$HarnessLogPath = Join-Path $LogDir "native-ptt.log"
$CargoTargetDir = Join-Path $RunRoot "cargo-target"
$AsrTargetDir = if ([string]::IsNullOrWhiteSpace($env:ATMOSPEAK_ASR_TARGET_DIR)) {
  "E:\asrs"
}
else {
  $env:ATMOSPEAK_ASR_TARGET_DIR
}
$AsrTargetDir = Assert-EPath -Path $AsrTargetDir -Label "ATMOSPEAK_ASR_TARGET_DIR"
$NpmCacheDir = Join-Path $RunRoot "npm-cache"
$WebViewUserDataDir = Join-Path $RunRoot "webview-user-data"
$ShaderCacheDir = Join-Path $RunRoot "shader-cache"
$NodeCompileCacheDir = Join-Path $RunRoot "node-compile-cache"
$CargoHome = "E:\BuildCache\cargo"
foreach ($directory in @($RunRoot, $TempRoot, $ProfileDir, $LogDir, $CargoTargetDir, $AsrTargetDir, $NpmCacheDir, $WebViewUserDataDir, $ShaderCacheDir, $NodeCompileCacheDir)) {
  New-Item -ItemType Directory -Force -Path $directory | Out-Null
}

if ([string]::IsNullOrWhiteSpace($FixturePath)) {
  $FixturePath = Join-Path $RunRoot "fixtures\ptt.wav"
  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $FixturePath) | Out-Null
  $GeneratedFixture = $true
}
else {
  $FixturePath = Assert-EPath -Path $FixturePath -Label "FixturePath" -MustExist
}
if (-not $GeneratedFixture -and -not (Test-Path -LiteralPath $FixturePath -PathType Leaf)) {
  throw "Push-to-talk audio fixture not found: $FixturePath"
}

$NodeExe = Get-Node24Path
$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$listener.Start()
$Port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
$listener.Stop()

$EnvironmentNames = @(
  "TEMP", "TMP", "CARGO_HOME", "CARGO_TARGET_DIR", "NPM_CONFIG_CACHE",
  "ATMOSPEAK_BUILD_TEMP", "ATMOSPEAK_APP_DATA_DIR", "ATMOSPEAK_WEBVIEW_DEBUG_PORT",
  "ATMOSPEAK_NATIVE_HARNESS", "ATMOSPEAK_TEST_AUDIO_FIXTURE", "ATMOSPEAK_ASR_BACKEND",
  "ATMOSPEAK_ASR_TARGET_DIR",
  "WEBVIEW2_USER_DATA_FOLDER", "__GL_SHADER_DISK_CACHE", "__GL_SHADER_DISK_CACHE_PATH",
  "NV_SHADER_DISK_CACHE", "NV_SHADER_DISK_CACHE_PATH", "NODE_COMPILE_CACHE"
)
$PreviousEnvironment = @{}
foreach ($name in $EnvironmentNames) {
  $PreviousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
}
$OwnedProcessIds = [System.Collections.Generic.HashSet[int]]::new()
$Process = $null
$DevServer = $null

try {
  $env:TEMP = $TempRoot
  $env:TMP = $TempRoot
  $env:CARGO_HOME = $CargoHome
  $env:CARGO_TARGET_DIR = $CargoTargetDir
  $env:NPM_CONFIG_CACHE = $NpmCacheDir
  $env:ATMOSPEAK_BUILD_TEMP = $TempRoot
  $env:ATMOSPEAK_ASR_TARGET_DIR = $AsrTargetDir
  $env:WEBVIEW2_USER_DATA_FOLDER = $WebViewUserDataDir
  $env:__GL_SHADER_DISK_CACHE = "1"
  $env:__GL_SHADER_DISK_CACHE_PATH = $ShaderCacheDir
  $env:NV_SHADER_DISK_CACHE = "1"
  $env:NV_SHADER_DISK_CACHE_PATH = $ShaderCacheDir
  $env:NODE_COMPILE_CACHE = $NodeCompileCacheDir
  $env:ATMOSPEAK_APP_DATA_DIR = $ProfileDir
  $env:ATMOSPEAK_WEBVIEW_DEBUG_PORT = "$Port"
  $env:ATMOSPEAK_NATIVE_HARNESS = "1"
  if ([string]::IsNullOrWhiteSpace($env:ATMOSPEAK_ASR_BACKEND)) {
    $env:ATMOSPEAK_ASR_BACKEND = "cpu"
  }
  if ($env:ATMOSPEAK_ASR_BACKEND -ne "cpu") {
    throw "The native smoke supports only the bundled Parakeet CPU engine; set ATMOSPEAK_ASR_BACKEND=cpu."
  }

  if ($GeneratedFixture) {
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
  $env:ATMOSPEAK_TEST_AUDIO_FIXTURE = (Resolve-Path -LiteralPath $FixturePath).Path

  if ($env:ATMOSPEAK_ASR_REBUILD -eq "1") {
    Write-Host "ATMOSPEAK_ASR_REBUILD=1 - rebuilding ASR sidecars under $AsrTargetDir."
    $buildArgs = @(
      "-NoProfile", "-ExecutionPolicy", "Bypass",
      "-File", (Join-Path $PSScriptRoot "build-asr-sidecars.ps1")
    )
    & powershell.exe @buildArgs
    if ($LASTEXITCODE -ne 0) {
      throw "ASR sidecar rebuild failed with code $LASTEXITCODE."
    }
  }

  $CpuSidecar = Join-Path $Root "src-tauri\resources\asr\atmospeak-asr-cpu.exe"
  if (-not (Test-Path -LiteralPath $CpuSidecar -PathType Leaf)) {
    throw "CPU ASR sidecar missing at $CpuSidecar - run scripts/build-asr-sidecars.ps1"
  }
  $tauriConfig = Get-Content -LiteralPath (Join-Path $Root "src-tauri/tauri.conf.json") -Raw | ConvertFrom-Json
  foreach ($relative in $tauriConfig.bundle.resources) {
    if (-not (Test-Path -Path (Join-Path $Root "src-tauri/$relative") -PathType Leaf)) {
      throw "Required bundle resource is missing: $relative"
    }
  }

  if (Test-ListeningPort -Port $DevServerPort) {
    throw "Refusing to start Vite: 127.0.0.1:$DevServerPort is already listening. Stop only the owning verification run or choose a clean lane."
  }
  $Bun = Get-Command bun -ErrorAction Stop
  $DevServer = Start-Process -FilePath $Bun.Source -WorkingDirectory $Root -ArgumentList @("run", "dev", "--", "--host", "127.0.0.1", "--port", "$DevServerPort", "--strictPort") -WindowStyle Hidden -PassThru
  [void]$OwnedProcessIds.Add([int]$DevServer.Id)

  $devDeadline = (Get-Date).AddSeconds(20)
  do {
    if ($DevServer.HasExited) {
      throw "Vite exited before the native harness could start (code $($DevServer.ExitCode))."
    }
    try {
      Invoke-WebRequest "http://127.0.0.1:$DevServerPort/" -TimeoutSec 1 -UseBasicParsing | Out-Null
      break
    }
    catch {
      Start-Sleep -Milliseconds 200
    }
  } while ((Get-Date) -lt $devDeadline)
  if ((Get-Date) -ge $devDeadline) {
    throw "Vite did not become ready within 20 seconds."
  }

  $Process = Start-Process -FilePath $AppExe -WorkingDirectory (Split-Path -Parent $AppExe) -WindowStyle Normal -PassThru
  [void]$OwnedProcessIds.Add([int]$Process.Id)
  Start-Sleep -Seconds 1
  if ($Process.HasExited) {
    throw "Atmospeak exited before WebView2 opened (code $($Process.ExitCode))."
  }

  Write-Host "Owned verification process IDs: $($OwnedProcessIds -join ', ')"
  & $NodeExe (Join-Path $PSScriptRoot "native-push-to-talk-harness.mjs") "--port=$Port" "--hotkey=$Hotkey" 2>&1 |
    Tee-Object -FilePath $HarnessLogPath
  $harnessExitCode = $LASTEXITCODE
  if ($harnessExitCode -ne 0) {
    throw "Native push-to-talk harness failed with code $harnessExitCode. See $HarnessLogPath"
  }
}
finally {
  $ownedRoots = @()
  if ($Process) {
    $ownedRoots += [int]$Process.Id
  }
  if ($DevServer) {
    $ownedRoots += [int]$DevServer.Id
  }
  if ($ownedRoots.Count -gt 0) {
    Stop-OwnedProcessTree -RootProcessIds $ownedRoots
  }
  foreach ($name in $EnvironmentNames) {
    $value = $PreviousEnvironment[$name]
    if ($null -eq $value) {
      Remove-Item "Env:$name" -ErrorAction SilentlyContinue
    }
    else {
      Set-Item "Env:$name" $value
    }
  }
  if (Test-Path -LiteralPath $ProfileDir) {
    Remove-Item -LiteralPath $ProfileDir -Recurse -Force -ErrorAction SilentlyContinue
  }
  if ($GeneratedFixture -and (Test-Path -LiteralPath $FixturePath)) {
    Remove-Item -LiteralPath $FixturePath -Force -ErrorAction SilentlyContinue
  }
}
