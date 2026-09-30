# Pure validation and presentation models used by the Windows Forms adapter.
# Keep the models free of WinForms types so they remain testable on non-Windows hosts.
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Presentation setters update transient GUI labels only.')]
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '', Justification = 'Plural nouns describe collections and established parameter names in private GUI helpers.')]
param()

function ConvertFrom-GuiCommaList {
  [CmdletBinding()]
  [OutputType([string[]])]
  param(
    [AllowEmptyString()]
    [string]$Text
  )

  return @($Text.Split(',') | ForEach-Object { $_.Trim() } | Where-Object { $_ })
}

function ConvertFrom-GuiTcpStreamsText {
  [CmdletBinding()]
  [OutputType([int[]])]
  param(
    [AllowEmptyString()]
    [string]$Text
  )

  $items = @(ConvertFrom-GuiCommaList -Text $Text)
  if ($items.Count -eq 0) {
    throw 'TCP streams requires at least one whole number from 1 to 128.'
  }
  $values = [System.Collections.Generic.List[int]]::new()
  foreach ($item in $items) {
    $value = 0
    if (-not [int]::TryParse($item, [Globalization.NumberStyles]::Integer, [Globalization.CultureInfo]::InvariantCulture, [ref]$value) -or
        $value -lt 1 -or $value -gt 128) {
      throw "TCP streams value '$item' must be a whole number from 1 to 128."
    }
    $values.Add($value)
  }
  return $values.ToArray()
}

function Test-GuiTargetName {
  [CmdletBinding()]
  [OutputType([bool])]
  param([AllowEmptyString()][string]$Name)

  if ([string]::IsNullOrWhiteSpace($Name) -or $Name.StartsWith('-')) { return $false }
  if ($Name -match '^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$') {
    foreach ($octet in $Name.Split('.')) {
      if ([int]$octet -gt 255) { return $false }
    }
    return $true
  }
  if ($Name -match '^[a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?)*$') {
    return $true
  }
  if ($Name.Contains('%')) { return $false }
  $trimmed = $Name.Trim()
  if (($trimmed.StartsWith('[') -and -not $trimmed.EndsWith(']')) -or
      ($trimmed.EndsWith(']') -and -not $trimmed.StartsWith('['))) {
    return $false
  }
  $parsedAddress = $null
  return [System.Net.IPAddress]::TryParse($trimmed.Trim('[', ']'), [ref]$parsedAddress)
}

function Get-GuiRunInputIssues {
  [CmdletBinding()]
  [OutputType([object[]])]
  param([Parameter(Mandatory)][hashtable]$Values)

  $issues = [System.Collections.Generic.List[object]]::new()
  $addIssue = {
    param([string]$Field, [string]$Message)
    $issues.Add([pscustomobject]@{ Field = $Field; Message = $Message })
  }

  $target = [string]$Values.Target
  if ([string]::IsNullOrWhiteSpace($target)) {
    & $addIssue 'Target' 'Target is required.'
  }
  elseif (-not (Test-GuiTargetName -Name $target.Trim())) {
    & $addIssue 'Target' 'Enter a valid hostname or IP address.'
  }

  $outDir = [string]$Values.OutDir
  if ([string]::IsNullOrWhiteSpace($outDir)) {
    & $addIssue 'OutDir' 'Output directory is required.'
  }
  elseif ($outDir -match '[\x00-\x1f]' -or $outDir.Trim().StartsWith('-')) {
    & $addIssue 'OutDir' 'Enter an output directory without control characters or a leading dash.'
  }

  $dscpValues = @(ConvertFrom-GuiCommaList -Text ([string]$Values.DscpClasses))
  if ($dscpValues.Count -eq 0) {
    & $addIssue 'DscpClasses' 'Add at least one DSCP class.'
  }
  else {
    foreach ($value in $dscpValues) {
      if ($value -cnotmatch '^(CS[0-7]|EF|AF[1-4][1-3])$') {
        & $addIssue 'DscpClasses' "DSCP '$value' is invalid. Use CS0-CS7, EF, or AF11-AF43."
        break
      }
    }
  }

  $windowValues = @(ConvertFrom-GuiCommaList -Text ([string]$Values.TcpWindows))
  if ($windowValues.Count -eq 0) {
    & $addIssue 'TcpWindows' 'Add at least one TCP window size.'
  }
  else {
    foreach ($value in $windowValues) {
      if ($value -notmatch '^(default|[0-9]+[kKmMgG]?)$') {
        & $addIssue 'TcpWindows' "TCP window '$value' is invalid. Use default or a whole number with an optional K, M, or G suffix."
        break
      }
    }
  }

  try { $null = ConvertFrom-GuiTcpStreamsText -Text ([string]$Values.TcpStreams) }
  catch { & $addIssue 'TcpStreams' $_.Exception.Message }

  foreach ($bandwidthField in @('UdpStart', 'UdpMax', 'UdpStep')) {
    $value = ([string]$Values[$bandwidthField]).Trim()
    if ($value -notmatch '^[0-9]+(?:\.[0-9]+)?\s*[kKmMgG]?$') {
      & $addIssue $bandwidthField "$bandwidthField must be a number with an optional K, M, or G suffix."
    }
  }

  return $issues.ToArray()
}

function Format-GuiNominalDuration {
  [CmdletBinding()]
  [OutputType([string])]
  param([ValidateRange(0, [long]::MaxValue)][long]$Seconds)

  $hours = [math]::Floor($Seconds / 3600)
  $minutes = [math]::Floor(($Seconds % 3600) / 60)
  $remainingSeconds = $Seconds % 60
  if ($hours -gt 0) { return ('{0}h {1}m {2}s' -f $hours, $minutes, $remainingSeconds) }
  if ($minutes -gt 0) { return ('{0}m {1}s' -f $minutes, $remainingSeconds) }
  return "${remainingSeconds}s"
}

function Get-GuiPlanSummaryModel {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [ValidateSet('Empty', 'Stale', 'Loading', 'Current', 'Error')]
    [string]$State = 'Empty',
    [AllowNull()]
    [object]$Preview,
    [string]$ErrorMessage
  )

  if ($State -eq 'Stale' -and -not $Preview) { $State = 'Empty' }
  switch ($State) {
    'Empty' {
      return [pscustomobject]@{
        State = $State; Tone = 'Neutral'; Headline = 'No plan preview yet'
        Detail = 'Preview the plan to confirm test count, nominal duration, and budget before running.'
      }
    }
    'Stale' {
      return [pscustomobject]@{
        State = $State; Tone = 'Warning'; Headline = 'Plan preview is out of date'
        Detail = 'Settings changed. Preview the plan again before relying on the previous estimate.'
      }
    }
    'Loading' {
      return [pscustomobject]@{
        State = $State; Tone = 'Neutral'; Headline = 'Calculating plan preview...'
        Detail = 'Capability discovery may take a moment. No throughput tests or output writes occur.'
      }
    }
    'Error' {
      $detail = if ([string]::IsNullOrWhiteSpace($ErrorMessage)) { 'Correct the highlighted settings and preview again.' } else { $ErrorMessage }
      return [pscustomobject]@{ State = $State; Tone = 'Error'; Headline = 'Plan preview failed'; Detail = $detail }
    }
  }

  if (-not $Preview -or $null -eq $Preview.TotalApprox -or $null -eq $Preview.EstimatedTestSeconds) {
    return Get-GuiPlanSummaryModel -State Error -ErrorMessage 'The preview did not return plan totals.'
  }
  $total = [long]$Preview.TotalApprox
  $max = [long]$Preview.MaxTotalTests
  $duration = Format-GuiNominalDuration -Seconds ([long]$Preview.EstimatedTestSeconds)
  if ([bool]$Preview.WithinTestBudget) {
    $budget = if ($max -eq 0) { 'No test limit is configured.' } else { "$total of $max allowed tests." }
    return [pscustomobject]@{
      State = 'Current'; Tone = 'Success'; Headline = "$total planned tests | $duration nominal"
      Detail = "Within test budget. $budget Estimate excludes setup and retries; UDP saturation may stop early."
    }
  }
  $overBy = $total - $max
  return [pscustomobject]@{
    State = 'Current'; Tone = 'Error'; Headline = "$total planned tests | $duration nominal"
    Detail = "Over budget by $overBy tests. Increase Max tests or reduce the test matrix before running."
  }
}

function Get-GuiProfileValueOrDefault {
  [CmdletBinding()]
  param(
    [Parameter(Mandatory)][object]$Parameters,
    [Parameter(Mandatory)][string]$Name,
    [AllowNull()][object]$Default
  )

  $property = $Parameters.PSObject.Properties[$Name]
  if (-not $property -or $null -eq $property.Value) { return $Default }
  return $property.Value
}

function Get-GuiRunSummaryModel {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param([AllowNull()][object]$Summary)

  if (-not $Summary) {
    return [pscustomobject]@{ Headline = 'No completed run yet'; Detail = 'Run results and report locations will appear here.'; Tone = 'Neutral' }
  }
  $status = [string]$Summary.Status
  $counts = $Summary.Counts
  $detail = if ($counts) {
    '{0} total | {1} passed | {2} failed' -f [int]$counts.Total, [int]$counts.Passed, [int]$counts.Failed
  }
  else { 'Open the generated artifacts below for full details.' }
  $tone = if ($status -eq 'Success') { 'Success' } elseif ($status -match 'Fail|Error') { 'Error' } else { 'Warning' }
  return [pscustomobject]@{ Headline = $status; Detail = $detail; Tone = $tone }
}

function Set-GuiPresentationLabels {
  param(
    [Parameter(Mandatory)][object]$Form,
    [Parameter(Mandatory)][string]$HeadlineName,
    [Parameter(Mandatory)][string]$DetailName,
    [Parameter(Mandatory)][object]$Model
  )

  $headline = $Form.Controls.Find($HeadlineName, $true) | Select-Object -First 1
  $detail = $Form.Controls.Find($DetailName, $true) | Select-Object -First 1
  if ($headline) { $headline.Text = [string]$Model.Headline }
  if ($detail) { $detail.Text = [string]$Model.Detail }
  if (-not ('System.Drawing.Color' -as [type])) { return }
  $toneColor = switch ($Model.Tone) {
    'Success' { [System.Drawing.ColorTranslator]::FromHtml('#147D6F') }
    'Warning' { [System.Drawing.ColorTranslator]::FromHtml('#8A5A00') }
    'Error' { [System.Drawing.ColorTranslator]::FromHtml('#B42318') }
    default { [System.Drawing.ColorTranslator]::FromHtml('#475467') }
  }
  if ($headline) { $headline.ForeColor = $toneColor }
}

function Set-GuiPlanSummaryState {
  param(
    [Parameter(Mandatory)][object]$Form,
    [ValidateSet('Empty', 'Stale', 'Loading', 'Current', 'Error')][string]$State,
    [AllowNull()][object]$Preview,
    [string]$ErrorMessage
  )

  $model = Get-GuiPlanSummaryModel -State $State -Preview $Preview -ErrorMessage $ErrorMessage
  Set-GuiPresentationLabels -Form $Form -HeadlineName 'lblPlanHeadline' -DetailName 'lblPlanDetail' -Model $model
}

function Set-GuiLastRunSummary {
  param(
    [Parameter(Mandatory)][object]$Form,
    [AllowNull()][object]$Summary
  )

  $model = Get-GuiRunSummaryModel -Summary $Summary
  Set-GuiPresentationLabels -Form $Form -HeadlineName 'lblRunResultHeadline' -DetailName 'lblRunResultDetail' -Model $model
}
