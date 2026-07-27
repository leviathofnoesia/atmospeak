param(
  [string]$ServerPath = "",
  [string]$ModelPath = ""
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($ServerPath)) {
  $ServerPath = Join-Path $Root "src-tauri\resources\whisper-runtime\whisper-server.exe"
}
if ([string]::IsNullOrWhiteSpace($ModelPath)) {
  $ModelPath = Join-Path $Root "src-tauri\resources\models\ggml-base.en.bin"
}
if (-not (Test-Path -LiteralPath $ServerPath)) {
  throw "Whisper server not found: $ServerPath"
}
if (-not (Test-Path -LiteralPath $ModelPath)) {
  throw "Whisper model not found: $ModelPath"
}

$listener = [System.Net.Sockets.TcpListener]::new(
  [System.Net.IPAddress]::Loopback,
  0
)
$listener.Start()
$Port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
$listener.Stop()

$FixturePath = Join-Path ([System.IO.Path]::GetTempPath()) (
  "atmospeak-host-check-{0}.wav" -f [Guid]::NewGuid().ToString("N")
)
$Phrase = "The porcelain moon hums over the studio."
$Server = $null

try {
  Add-Type -AssemblyName System.Speech
  $synthesizer = [System.Speech.Synthesis.SpeechSynthesizer]::new()
  try {
    $synthesizer.SetOutputToWaveFile($FixturePath)
    $synthesizer.Speak($Phrase)
  }
  finally {
    $synthesizer.Dispose()
  }

  $Server = Start-Process -FilePath $ServerPath `
    -WorkingDirectory (Split-Path -Parent $ServerPath) `
    -ArgumentList @("-m", $ModelPath, "--host", "127.0.0.1", "--port", $Port) `
    -WindowStyle Hidden `
    -PassThru

  $deadline = (Get-Date).AddSeconds(30)
  do {
    if ($Server.HasExited) {
      throw "whisper-server exited during startup with code $($Server.ExitCode)"
    }
    try {
      Invoke-WebRequest "http://127.0.0.1:$Port/" -TimeoutSec 2 -UseBasicParsing |
        Out-Null
      break
    }
    catch {
      Start-Sleep -Milliseconds 150
    }
  } while ((Get-Date) -lt $deadline)

  if ((Get-Date) -ge $deadline) {
    throw "whisper-server did not become ready within 30 seconds"
  }

  $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
  $Transcript = & curl.exe --silent --show-error --fail `
    -F "file=@$FixturePath;type=audio/wav" `
    -F "response_format=text" `
    "http://127.0.0.1:$Port/inference"
  $stopwatch.Stop()
  if ($LASTEXITCODE -ne 0) {
    throw "whisper-server request failed with code $LASTEXITCODE"
  }
  $TranscriptText = ($Transcript -join " ").Trim()
  foreach ($requiredToken in @("porcelain", "moon", "studio")) {
    if ($TranscriptText -notmatch [Regex]::Escape($requiredToken)) {
      throw "Host transcript did not contain '$requiredToken': $TranscriptText"
    }
  }

  Write-Host "Host transcription passed in $($stopwatch.ElapsedMilliseconds)ms."
  Write-Host "Transcript: $TranscriptText"
}
finally {
  if ($Server -and -not $Server.HasExited) {
    Stop-Process -Id $Server.Id -Force -ErrorAction SilentlyContinue
    Wait-Process -Id $Server.Id -Timeout 10 -ErrorAction SilentlyContinue
  }
  Remove-Item -LiteralPath $FixturePath -Force -ErrorAction SilentlyContinue
}
