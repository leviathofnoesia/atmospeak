param(
  [Parameter(Mandatory = $true)]
  [string]$OutputPath,

  [Parameter(Mandatory = $true)]
  [string]$ReadyPath
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$form = New-Object System.Windows.Forms.Form
$form.Text = "Native Injection Target"
$form.Width = 720
$form.Height = 360
$form.StartPosition = "CenterScreen"

$editor = New-Object System.Windows.Forms.TextBox
$editor.Multiline = $true
$editor.AcceptsReturn = $true
$editor.AcceptsTab = $true
$editor.Dock = "Fill"
$editor.Font = New-Object System.Drawing.Font("Segoe UI", 14)
$form.Controls.Add($editor)

$encoding = [System.Text.UTF8Encoding]::new($false)
function Write-EditorSnapshot {
  try {
    [System.IO.File]::WriteAllText($OutputPath, $editor.Text, $encoding)
  } catch {
    # Best-effort; the harness also polls. Never break the UI thread on IO races.
  }
}
$editor.Add_TextChanged({ Write-EditorSnapshot })
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
