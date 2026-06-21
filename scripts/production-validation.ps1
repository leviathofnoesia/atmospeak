param(
  [string]$MatrixPath = "tests/manual/production-100.md",
  [string]$RunLogPath = "tests/manual/production-run-log.md",
  [switch]$AsJson,
  [switch]$ListPending,
  [string[]]$NewRunIds = @(),
  [switch]$AppendTemplates
)

$ErrorActionPreference = "Stop"

function Resolve-RepoPath {
  param([string]$Path)
  if ([System.IO.Path]::IsPathRooted($Path)) {
    return $Path
  }
  return Join-Path (Get-Location) $Path
}

function Read-RequiredText {
  param([string]$Path)
  $resolved = Resolve-RepoPath $Path
  if (-not (Test-Path -LiteralPath $resolved)) {
    throw "Required validation file not found: $resolved"
  }
  return Get-Content -Raw -LiteralPath $resolved
}

function Get-MatrixCases {
  param([string]$Text)
  $rows = [regex]::Matches($Text, '(?m)^\|\s*(\d{3})\s*\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|$')
  $cases = foreach ($row in $rows) {
    [pscustomobject]@{
      Id = $row.Groups[1].Value
      Area = $row.Groups[2].Value.Trim()
      Scenario = $row.Groups[3].Value.Trim()
      DataToCapture = $row.Groups[4].Value.Trim()
      PassCriteria = $row.Groups[5].Value.Trim()
    }
  }
  return @($cases)
}

function Get-RunIds {
  param([string]$Text)
  $ids = New-Object System.Collections.Generic.HashSet[string]
  foreach ($entry in (Get-RunEntries $Text)) {
    [void]$ids.Add($entry.Id)
  }
  return ,$ids
}

function Get-RunEntries {
  param([string]$Text)
  $headingPattern = '(?mi)^###\s+(RUN-[^\r\n]*?)\s+-\s*(?:Test\s+)?(\d{3})\b[^\r\n]*'
  $matches = @([regex]::Matches($Text, $headingPattern))
  $entries = for ($index = 0; $index -lt $matches.Count; $index += 1) {
    $match = $matches[$index]
    $start = $match.Index
    $end = if ($index + 1 -lt $matches.Count) { $matches[$index + 1].Index } else { $Text.Length }
    $body = $Text.Substring($start, $end - $start)
    [pscustomobject]@{
      RunId = $match.Groups[1].Value.Trim()
      Id = $match.Groups[2].Value
      Body = $body
      Fields = Get-RunEntryFields $body
    }
  }
  return @($entries)
}

function Get-RunEntryFields {
  param([string]$Body)
  $fields = @{}
  foreach ($line in ($Body -split "`r?`n")) {
    if ($line -match '^\s*-\s*([^:]+):\s*(.*)$') {
      $fields[$Matches[1].Trim()] = $Matches[2].Trim()
    }
  }
  return $fields
}

function Get-FieldValue {
  param(
    [hashtable]$Fields,
    [string]$Name
  )
  if (-not $Fields.ContainsKey($Name)) {
    return ""
  }
  return [string]$Fields[$Name]
}

function Test-RunEntryComplete {
  param([pscustomobject]$Entry)
  $status = (Get-FieldValue $Entry.Fields "Status").ToLowerInvariant()
  $completionStatuses = @("pass", "minor issue", "fail", "blocked")
  if ($status -notin $completionStatuses) {
    return $false
  }
  foreach ($field in $script:RequiredEvidenceFields) {
    if ([string]::IsNullOrWhiteSpace((Get-FieldValue $Entry.Fields $field))) {
      return $false
    }
  }
  return $true
}

function Get-MalformedCompletedRuns {
  param([array]$Entries)
  $completionStatuses = @("pass", "minor issue", "fail", "blocked")
  $bad = foreach ($entry in $Entries) {
    $status = (Get-FieldValue $entry.Fields "Status").ToLowerInvariant()
    if ($status -in $completionStatuses -and -not (Test-RunEntryComplete $entry)) {
      $missing = @(
        $script:RequiredEvidenceFields |
          Where-Object { [string]::IsNullOrWhiteSpace((Get-FieldValue $entry.Fields $_)) }
      )
      [pscustomobject]@{
        RunId = $entry.RunId
        Id = $entry.Id
        Status = $status
        MissingFields = $missing
      }
    }
  }
  return ,@($bad)
}

function Normalize-RequestedIds {
  param([string[]]$Ids)
  $normalized = foreach ($item in $Ids) {
    foreach ($part in ($item -split '[,\s]+')) {
      $trimmed = $part.Trim()
      if ($trimmed.Length -eq 0) {
        continue
      }
      if ($trimmed -notmatch '^\d{1,3}$') {
        throw "Invalid test id: $trimmed"
      }
      ([int]$trimmed).ToString("000")
    }
  }
  return @($normalized | Select-Object -Unique)
}

function New-RunTemplate {
  param(
    [pscustomobject]$Case,
    [string]$RunStamp
  )
  return @"
### RUN-$RunStamp - Test $($Case.Id)

- Status: started
- Environment:
- App version/commit:
- Target app:
- Settings changed:
- Audio/session id:
- Audio path:
- Raw transcript:
- Cleaned transcript:
- Target output:
- Notices/runtime events:
- Latency/performance:
- Accuracy:
- Correct feature use:
- Conciseness/completeness:
- Recovery:
- UI/UX smoothness:
- Performance:
- Issues found:
- Fix applied:
- Retest link:

Matrix scenario: $($Case.Scenario)
Data to capture: $($Case.DataToCapture)
Pass criteria: $($Case.PassCriteria)

"@
}

$matrixText = Read-RequiredText $MatrixPath
$runLogText = Read-RequiredText $RunLogPath
$cases = Get-MatrixCases $matrixText
$entries = Get-RunEntries $runLogText
$runIds = Get-RunIds $runLogText
$script:RequiredEvidenceFields = @(
  "Environment",
  "App version/commit",
  "Accuracy",
  "Correct feature use",
  "Conciseness/completeness",
  "Recovery",
  "UI/UX smoothness",
  "Performance"
)

$requestedTemplateIds = Normalize-RequestedIds $NewRunIds
if ($requestedTemplateIds.Count -gt 0) {
  $caseById = @{}
  foreach ($case in $cases) {
    $caseById[$case.Id] = $case
  }
  $runStamp = Get-Date -Format "yyyyMMdd-HHmm"
  $templates = foreach ($id in $requestedTemplateIds) {
    if (-not $caseById.ContainsKey($id)) {
      throw "Cannot create run template for unknown test id: $id"
    }
    New-RunTemplate -Case $caseById[$id] -RunStamp $runStamp
  }
  $templateText = ($templates -join "")
  if ($AppendTemplates) {
    $resolvedRunLogPath = Resolve-RepoPath $RunLogPath
    Add-Content -LiteralPath $resolvedRunLogPath -Value "`r`n$templateText"
    Write-Host "Appended $($requestedTemplateIds.Count) run template(s) to $RunLogPath"
  } else {
    Write-Output $templateText
  }
  exit 0
}

$expectedIds = 1..100 | ForEach-Object { $_.ToString("000") }
$actualIds = @($cases | ForEach-Object { $_.Id })
$missingIds = @($expectedIds | Where-Object { $_ -notin $actualIds })
$extraIds = @($actualIds | Where-Object { $_ -notin $expectedIds })
$duplicateIds = @(
  $actualIds |
    Group-Object |
    Where-Object { $_.Count -gt 1 } |
    ForEach-Object { $_.Name }
)

$startedIds = @($expectedIds | Where-Object { $runIds.Contains($_) })
$completeEntries = @($entries | Where-Object { Test-RunEntryComplete $_ })
$completeIdSet = New-Object System.Collections.Generic.HashSet[string]
foreach ($entry in $completeEntries) {
  [void]$completeIdSet.Add($entry.Id)
}
$completedIds = @($expectedIds | Where-Object { $completeIdSet.Contains($_) })
$pendingIds = @($expectedIds | Where-Object { -not $completeIdSet.Contains($_) })
$malformedCompletedRuns = Get-MalformedCompletedRuns $entries
$areaCounts = @{}
foreach ($case in $cases) {
  if (-not $areaCounts.ContainsKey($case.Area)) {
    $areaCounts[$case.Area] = 0
  }
  $areaCounts[$case.Area] += 1
}

$valid = $cases.Count -eq 100 -and $missingIds.Count -eq 0 -and $extraIds.Count -eq 0 -and $duplicateIds.Count -eq 0 -and $malformedCompletedRuns.Count -eq 0
$summary = [ordered]@{
  valid = $valid
  matrixPath = $MatrixPath
  runLogPath = $RunLogPath
  caseCount = $cases.Count
  runEntryCount = $entries.Count
  startedCount = $startedIds.Count
  completedCount = $completedIds.Count
  pendingCount = $pendingIds.Count
  missingIds = $missingIds
  extraIds = $extraIds
  duplicateIds = $duplicateIds
  malformedCompletedRuns = $malformedCompletedRuns
  areaCounts = $areaCounts
  startedIds = $startedIds
  completedIds = $completedIds
  pendingIds = $pendingIds
}

if ($AsJson) {
  $summary | ConvertTo-Json -Depth 5
} else {
  Write-Host "Atmospeak production validation matrix"
  Write-Host "  Matrix:   $MatrixPath"
  Write-Host "  Run log:  $RunLogPath"
  Write-Host "  Valid:    $valid"
  Write-Host "  Cases:    $($cases.Count)"
  Write-Host "  Entries:  $($entries.Count)"
  Write-Host "  Started:  $($startedIds.Count)"
  Write-Host "  Complete: $($completedIds.Count)"
  Write-Host "  Pending:  $($pendingIds.Count)"
  Write-Host ""
  Write-Host "Areas:"
  foreach ($area in ($areaCounts.Keys | Sort-Object)) {
    Write-Host ("  {0}: {1}" -f $area, $areaCounts[$area])
  }
  if ($missingIds.Count -gt 0) {
    Write-Host ""
    Write-Host "Missing IDs: $($missingIds -join ', ')"
  }
  if ($duplicateIds.Count -gt 0) {
    Write-Host ""
    Write-Host "Duplicate IDs: $($duplicateIds -join ', ')"
  }
  if ($malformedCompletedRuns.Count -gt 0) {
    Write-Host ""
    Write-Host "Malformed completed runs:"
    foreach ($bad in $malformedCompletedRuns) {
      Write-Host ("  {0} / {1}: missing {2}" -f $bad.RunId, $bad.Id, ($bad.MissingFields -join ', '))
    }
  }
  if ($ListPending) {
    Write-Host ""
    Write-Host "Pending IDs: $($pendingIds -join ', ')"
  }
}

if (-not $valid) {
  exit 1
}
