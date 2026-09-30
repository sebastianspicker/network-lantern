function Protect-DiagnosticCsvValue {
  param(
    [AllowNull()]
    [AllowEmptyString()]
    [string]$Value
  )

  if ([string]::IsNullOrEmpty($Value)) { return $Value }
  if ($Value[0] -in @('=', '+', '-', '@')) { return "'$Value" }
  return $Value
}

function New-DiagnosticArtifactTemporaryPath {
  [CmdletBinding()]
  param([Parameter(Mandatory)][string]$DestinationPath)

  $directory = [System.IO.Path]::GetDirectoryName($DestinationPath)
  $fileName = [System.IO.Path]::GetFileName($DestinationPath)
  return Join-Path $directory (".{0}.{1}.tmp" -f $fileName, [guid]::NewGuid().ToString('N'))
}

function Write-DiagnosticArtifactTemporaryFile {
  [CmdletBinding()]
  param(
    [Parameter(Mandatory)][System.IO.FileStream]$Stream,
    [Parameter(Mandatory)][AllowEmptyString()][string]$Content,
    [Parameter(Mandatory)][System.Text.Encoding]$Encoding
  )

  $bytes = $Encoding.GetBytes($Content)
  $Stream.Write($bytes, 0, $bytes.Length)
  $Stream.Flush($true)
}

function Move-DiagnosticArtifactIntoPlace {
  [CmdletBinding()]
  param(
    [Parameter(Mandatory)][string]$TemporaryPath,
    [Parameter(Mandatory)][string]$DestinationPath
  )

  if ([System.IO.File]::Exists($DestinationPath)) {
    [System.IO.File]::Move($TemporaryPath, $DestinationPath, $true)
  } else {
    [System.IO.File]::Move($TemporaryPath, $DestinationPath)
  }
}

function Publish-DiagnosticArtifact {
  [CmdletBinding()]
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][AllowEmptyString()][string]$Content,
    [Parameter(Mandatory)][System.Text.Encoding]$Encoding
  )

  $fullPath = [System.IO.Path]::GetFullPath($Path)
  $temporaryPath = New-DiagnosticArtifactTemporaryPath -DestinationPath $fullPath
  $stream = [System.IO.FileStream]::new(
    $temporaryPath,
    [System.IO.FileMode]::CreateNew,
    [System.IO.FileAccess]::Write,
    [System.IO.FileShare]::None
  )
  try {
    try {
      Write-DiagnosticArtifactTemporaryFile -Stream $stream -Content $Content -Encoding $Encoding
    } finally {
      $stream.Dispose()
    }
    Move-DiagnosticArtifactIntoPlace -TemporaryPath $temporaryPath -DestinationPath $fullPath
    $temporaryPath = $null
  } finally {
    if ($temporaryPath -and [System.IO.File]::Exists($temporaryPath)) {
      [System.IO.File]::Delete($temporaryPath)
    }
  }
}

function Save-DiagnosticResults {
  <#
  .SYNOPSIS
    Persist diagnostic results to JSON and CSV files (UTF-8 no BOM).
  .PARAMETER Results
    Array of diagnostic result objects to serialize.
  .PARAMETER JsonPath
    Output path for the JSON file.
  .PARAMETER CsvPath
    Output path for the CSV summary file.
  #>
  param(
    [Parameter(Mandatory)][AllowEmptyCollection()][object[]]$Results,
    [Parameter(Mandatory)][string]$JsonPath,
    [Parameter(Mandatory)][string]$CsvPath
  )

  $utf8NoBom = [System.Text.UTF8Encoding]::new($false)
  $jsonContent = ConvertTo-Json -InputObject @($Results) -Depth 6
  Publish-DiagnosticArtifact -Path $JsonPath -Content $jsonContent -Encoding $utf8NoBom

  $csvLines = $Results | Select-Object Timestamp, Round, Protocol, @{ Name = 'Host'; Expression = { Protect-DiagnosticCsvValue $_.Host } }, PingStatus, TracertStatus, PathpingStatus, Tcp443Status, PortsStatus, OverallStatus, PingOk, TracertOk, PathpingOk, Tcp443OK | ConvertTo-Csv -NoTypeInformation
  $csvContent = $csvLines -join [Environment]::NewLine
  Publish-DiagnosticArtifact -Path $CsvPath -Content $csvContent -Encoding $utf8NoBom

  Write-Status -Level SUMMARY -Message "JSON: $JsonPath"
  Write-Status -Level SUMMARY -Message "CSV : $CsvPath"
}
