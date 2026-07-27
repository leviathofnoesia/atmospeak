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
$editor.Add_TextChanged({
  [System.IO.File]::WriteAllText($OutputPath, $editor.Text, $encoding)
})
$form.Add_Activated({
  $editor.Focus() | Out-Null
})
$form.Add_Shown({
  $form.Activate()
  $editor.Focus() | Out-Null
  [System.IO.File]::WriteAllText($ReadyPath, "ready", $encoding)
})

[System.Windows.Forms.Application]::Run($form)
