param(
  [Parameter(Mandatory = $true)]
  [ValidatePattern('^\d+\.\d+\.\d+([-.][0-9A-Za-z.-]+)?$')]
  [string]$Version
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Utf8 = [System.Text.UTF8Encoding]::new($false)

function Update-FirstMatch {
  param(
    [string]$Path,
    [string]$Pattern,
    [string]$Replacement
  )

  $FullPath = Join-Path $Root $Path
  $Content = [System.IO.File]::ReadAllText($FullPath)
  $Regex = [System.Text.RegularExpressions.Regex]::new($Pattern)
  if (-not $Regex.IsMatch($Content)) {
    throw "Version pattern not found in $Path"
  }
  $Updated = $Regex.Replace($Content, $Replacement, 1)
  [System.IO.File]::WriteAllText($FullPath, $Updated, $Utf8)
}

Update-FirstMatch "package.json" '("version"\s*:\s*")[^"]+(")' "`${1}$Version`${2}"
Update-FirstMatch "src-tauri\Cargo.toml" '(?m)^(version\s*=\s*")[^"]+(")' "`${1}$Version`${2}"
Update-FirstMatch "src-tauri\tauri.conf.json" '("version"\s*:\s*")[^"]+(")' "`${1}$Version`${2}"
Update-FirstMatch "website\src\version.ts" '(APP_VERSION\s*=\s*")[^"]+(")' "`${1}$Version`${2}"

Write-Host "Atmospeak version set to $Version"
