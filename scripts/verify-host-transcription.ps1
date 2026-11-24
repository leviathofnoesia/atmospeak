#Requires -Version 5.1
<#
.SYNOPSIS
  Verify the packaged CPU Parakeet host with a deterministic WAV fixture.

.DESCRIPTION
  The host contract is one executable invocation:
    atmospeak-asr-cpu.exe --model <gguf> --transcribe-wav <file>
  This check deliberately does not start a server, microphone, browser, or
  alternate ASR engine.
#>
[CmdletBinding()]
param(
  [int]$Iterations = 1,
  [string]$ReportPath = "",
  [string]$SidecarPath = "",
  [string]$ModelPath = "",
  [string]$FixturePath = "",
  [string]$RunRoot = ""
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$DefaultRunRoot = "E:\BuildCache\atmospeak-smoke\run-data\parakeet-host"
$GeneratedFixture = $false
$OwnedProcessIds = New-Object 'System.Collections.Generic.HashSet[int]'
$modelHash = "4b50b6dd862bf6e346929aaf4f5eaacec003bfa3f56462d6c874b41ef2f38795"

function Assert-EPath {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string]$Label,
    [switch]$MustExist,
    [switch]$Directory
  )
  $full = [System.IO.Path]::GetFullPath($Path)
  if ($full -notmatch "^[Ee]:\\") {
    throw "$Label must resolve under E: for an isolated host check: $full"
  }
  if ($MustExist) {
    $kind = if ($Directory) { "Container" } else { "Leaf" }
    if (-not (Test-Path -LiteralPath $full -PathType $kind)) {
      throw "$Label does not exist: $full"
    }
  }
  return $full
}

function Get-Sha256Hex {
  param([Parameter(Mandatory = $true)][string]$Path)
  return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

function Assert-Model {
  param([Parameter(Mandatory = $true)][string]$Path)
  $file = Get-Item -LiteralPath $Path
  $hash = Get-Sha256Hex $Path
  if ($file.Length -ne 731357568 -or $hash -ne $modelHash) {
    throw "Parakeet weights do not match the pinned artifact."
  }
  [ordered]@{ name = $file.Name; size = $file.Length; sha256 = $hash }
}

function Assert-Transcript {
  param([Parameter(Mandatory = $true)][string]$Text)
  foreach ($token in @("porcelain", "moon", "studio")) {
    if ($Text -notmatch [Regex]::Escape($token)) {
      throw "Parakeet transcript did not contain '$token': $Text"
    }
  }
}

function Get-Percentile {
  param([Parameter(Mandatory = $true)][double[]]$Values, [Parameter(Mandatory = $true)][double]$Percentile)
  $sorted = @($Values | Sort-Object)
  $rank = [Math]::Max(1, [Math]::Ceiling(($Percentile / 100) * $sorted.Count))
  return [double]$sorted[$rank - 1]
}

if ($Iterations -lt 1 -or $Iterations -gt 10) {
  throw "Iterations must be between 1 and 10."
}
$RunRoot = Assert-EPath $(if ($RunRoot) { $RunRoot } else { $DefaultRunRoot }) "RunRoot"
$RunRoot = [System.IO.Path]::GetFullPath($RunRoot)
$SidecarPath = Assert-EPath $(if ($SidecarPath) { $SidecarPath } else { Join-Path $Root "src-tauri\resources\asr\atmospeak-asr-cpu.exe" }) "ASR sidecar" -MustExist
$ModelPath = Assert-EPath $(if ($ModelPath) { $ModelPath } else { Join-Path $Root "src-tauri\resources\models\parakeet-unified-en-0.6b-Q8_0.gguf" }) "Parakeet model" -MustExist
if ($ReportPath) {
  $ReportPath = Assert-EPath $ReportPath "ReportPath"
} else {
  $ReportPath = Join-Path $RunRoot "parakeet-host-report.json"
}
$ReportPath = [System.IO.Path]::GetFullPath($ReportPath)
$SidecarDirectory = Split-Path -Parent $SidecarPath
foreach ($dependency in @("transcribe.dll", "ggml.dll", "ggml-base.dll", "ggml-cpu-x64.dll")) {
  if (-not (Test-Path -LiteralPath (Join-Path $SidecarDirectory $dependency) -PathType Leaf)) {
    throw "Missing Parakeet native dependency: $dependency"
  }
}

$modelRecords = Assert-Model $ModelPath
New-Item -ItemType Directory -Force -Path $RunRoot | Out-Null
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $ReportPath) | Out-Null

if (-not $FixturePath) {
  $FixturePath = Join-Path $RunRoot "porcelain-moon-studio.wav"
  Add-Type -AssemblyName System.Speech
  $voice = New-Object System.Speech.Synthesis.SpeechSynthesizer
  try {
    $voice.Rate = -1
    $voice.Volume = 60
    $voice.SetOutputToWaveFile($FixturePath)
    $voice.Speak("The porcelain moon hums over the studio.")
  } finally {
    $voice.Dispose()
  }
  $GeneratedFixture = $true
} else {
  $FixturePath = Assert-EPath $FixturePath "FixturePath" -MustExist
}
$FixturePath = [System.IO.Path]::GetFullPath($FixturePath)

$previousTemp = $env:TEMP
$previousTmp = $env:TMP
$env:TEMP = Join-Path $RunRoot "tmp"
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null
$results = New-Object System.Collections.Generic.List[object]
try {
  for ($iteration = 1; $iteration -le $Iterations; $iteration += 1) {
    $stdoutPath = Join-Path $RunRoot ("stdout-{0}.txt" -f $iteration)
    $stderrPath = Join-Path $RunRoot ("stderr-{0}.txt" -f $iteration)
    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    $arguments = @("--model", ('"{0}"' -f $ModelPath), "--transcribe-wav", ('"{0}"' -f $FixturePath))
    $process = Start-Process -FilePath $SidecarPath -WorkingDirectory $SidecarDirectory -ArgumentList $arguments -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath -PassThru -WindowStyle Hidden
    $null = $process.Handle
    [void]$OwnedProcessIds.Add([int]$process.Id)
    if (-not $process.WaitForExit(180000)) {
      Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
      throw "Parakeet CPU sidecar timed out on iteration $iteration."
    }
    [void]$OwnedProcessIds.Remove([int]$process.Id)
    $timer.Stop()
    if ($process.ExitCode -ne 0) {
      $stderr = if (Test-Path -LiteralPath $stderrPath) { Get-Content -LiteralPath $stderrPath -Raw } else { "" }
      throw "Parakeet CPU sidecar exited with code $($process.ExitCode): $stderr"
    }
    $transcript = (Get-Content -LiteralPath $stdoutPath -Raw).Trim()
    Assert-Transcript $transcript
    [void]$results.Add([ordered]@{
      iteration = $iteration
      elapsedMs = [double]$timer.Elapsed.TotalMilliseconds
      transcript = $transcript
      stdout = $stdoutPath
      stderr = $stderrPath
    })
  }
} finally {
  foreach ($processId in @($OwnedProcessIds)) {
    Stop-Process -Id $processId -Force -ErrorAction SilentlyContinue
  }
  if ($null -eq $previousTemp) { Remove-Item Env:TEMP -ErrorAction SilentlyContinue } else { $env:TEMP = $previousTemp }
  if ($null -eq $previousTmp) { Remove-Item Env:TMP -ErrorAction SilentlyContinue } else { $env:TMP = $previousTmp }
}
$elapsed = @($results | ForEach-Object { [double]$_.elapsedMs })
$report = [ordered]@{
  passed = $true
  engine = "parakeet"
  backend = "cpu"
  modelId = "parakeet-unified-en"
  sidecar = $SidecarPath
  sidecarSha256 = Get-Sha256Hex $SidecarPath
  decoderSha256 = Get-Sha256Hex (Join-Path $SidecarDirectory "transcribe.dll")
  model = $modelRecords
  fixture = $FixturePath
  iterations = $Iterations
  elapsedMs = [ordered]@{
    p50 = Get-Percentile $elapsed 50
    p95 = Get-Percentile $elapsed 95
  }
  runs = $results.ToArray()
}
$report | ConvertTo-Json -Depth 16 | Set-Content -LiteralPath $ReportPath -Encoding utf8
Write-Host "Parakeet CPU host check passed. Report: $ReportPath"
if ($GeneratedFixture -and (Test-Path -LiteralPath $FixturePath)) {
  Remove-Item -LiteralPath $FixturePath -Force
}
