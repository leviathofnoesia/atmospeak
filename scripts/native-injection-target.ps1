param(
  [Parameter(Mandatory = $true)]
  [string]$OutputPath,

  [Parameter(Mandatory = $true)]
  [string]$ReadyPath,

  [switch]$RichText,

  [string]$EventLogPath = ""
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$form = New-Object System.Windows.Forms.Form
$form.Text = "Native Injection Target"
$form.Width = 720
$form.Height = 360
$form.StartPosition = "CenterScreen"

$editor = if ($RichText) { New-Object System.Windows.Forms.RichTextBox } else { New-Object System.Windows.Forms.TextBox }
$editor.Multiline = $true
if (-not $RichText) { $editor.AcceptsReturn = $true }
$editor.AcceptsTab = $true
$editor.Dock = "Fill"
$editor.Font = New-Object System.Drawing.Font("Segoe UI", 14)
$form.Controls.Add($editor)

$encoding = [System.Text.UTF8Encoding]::new($false)
function Write-EditorSnapshot([bool]$Changed = $false) {
  $visibleTicks = [System.Diagnostics.Stopwatch]::GetTimestamp()
  $visibleText = $editor.Text
  try {
    [System.IO.File]::WriteAllText($OutputPath, $visibleText, $encoding)
    $focusState = "$($form.ContainsFocus)|$($editor.Focused)"
    if ($EventLogPath -and $script:lastFocusState -ne $focusState) {
      $focusEvent = @{ kind = "focus-state"; ticks = "$visibleTicks"; formFocused = $form.ContainsFocus; editorFocused = $editor.Focused } | ConvertTo-Json -Compress
      [System.IO.File]::AppendAllText($EventLogPath, $focusEvent + "`n", $encoding)
      $script:lastFocusState = $focusState
    }
    if ($Changed -and $EventLogPath) {
      $sha = [System.Security.Cryptography.SHA256]::Create()
      try { $hash = [BitConverter]::ToString($sha.ComputeHash($encoding.GetBytes($visibleText))).Replace("-", "").ToLowerInvariant() } finally { $sha.Dispose() }
      $event = @{ kind = "text-changed"; ticks = "$visibleTicks"; frequency = "$( [System.Diagnostics.Stopwatch]::Frequency )"; length = $visibleText.Length; sha256 = $hash } | ConvertTo-Json -Compress
      [System.IO.File]::AppendAllText($EventLogPath, $event + "`n", $encoding)
    }
  } catch {
    # Best-effort; the harness also polls. Never break the UI thread on IO races.
    Write-Warning "Write-EditorSnapshot failed for '${OutputPath}': $($_.Exception.Message)"
  }
}
$editor.Add_TextChanged({ Write-EditorSnapshot $true })
$editor.Add_KeyDown({
  param($sender, $keyEvent)
  if ($EventLogPath -and $keyEvent.KeyCode -eq [System.Windows.Forms.Keys]::V) {
    $key = @{ kind = "paste-key"; ticks = "$([System.Diagnostics.Stopwatch]::GetTimestamp())"; control = $keyEvent.Control; shift = $keyEvent.Shift; alt = $keyEvent.Alt } | ConvertTo-Json -Compress
    [System.IO.File]::AppendAllText($EventLogPath, $key + "`n", $encoding)
  }
})
# Periodic flush so a paste that races TextChanged/file locks still becomes visible
# to the harness — do not treat an empty poll file as proof the paste failed.
$flushTimer = New-Object System.Windows.Forms.Timer
$flushTimer.Interval = 100
$flushTimer.Add_Tick({ Write-EditorSnapshot })
$flushTimer.Start()
$form.Add_Activated({
  $editor.Focus() | Out-Null
})
$form.Add_Shown({
  $form.Activate()
  $editor.Focus() | Out-Null
  [System.IO.File]::WriteAllText($ReadyPath, $form.Handle.ToInt64().ToString(), $encoding)
  Write-EditorSnapshot
})
$form.Add_FormClosed({
  $flushTimer.Stop()
  $flushTimer.Dispose()
})

[System.Windows.Forms.Application]::Run($form)
