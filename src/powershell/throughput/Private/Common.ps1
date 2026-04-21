# Common utility helpers (private to Iperf3TestSuite)

# --- Named constants ---
# Exit codes map to structured ErrorIds (see ErrorClassification.ps1).
# The CLI script (iPerf3Test.ps1) mirrors these values for process exit codes.
$script:ExitCodes = @{
  Success          = 0
  InputValidation  = 11
  Prerequisite     = 12
  Connectivity     = 13
  PartialFailure   = 14
  TotalFailure     = 15
  Internal         = 16
}
$script:MaxUdpSaturationIterations = 1000
$script:MaxJsonTextLength          = 1MB   # 1048576 chars
$script:MaxTopFailures             = 10
$script:DefaultTraceHops           = 5
$script:InvariantCulture           = [System.Globalization.CultureInfo]::InvariantCulture
$script:Iperf3ProcessTimeoutBufferSec = 30  # extra seconds beyond Duration+Omit before killing iperf3
$script:DefaultRetryCount            = 0   # retries per test on transient failure (ExitCode != 0, no JSON)
$script:RetryDelayMs                 = 2000
$script:ProfilesFileLockTimeoutMs    = 3000
# Threshold defaults: $null means "no threshold check" (disabled).
$script:DefaultThresholdMinThroughputMbps = $null
$script:DefaultThresholdMaxLossPct        = $null
$script:DefaultThresholdMaxJitterMs       = $null

function Test-PathUnderBase {
  [CmdletBinding()]
  [OutputType([bool])]
  param(
    [Parameter(Mandatory)]
    [string]$BasePath,
    [Parameter(Mandatory)]
    [string]$CandidatePath
  )
  $baseFull = [System.IO.Path]::GetFullPath($BasePath)
  $candidateFull = [System.IO.Path]::GetFullPath($CandidatePath)
  $separators = @([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
  $baseWithSeparator = $baseFull.TrimEnd($separators) + [System.IO.Path]::DirectorySeparatorChar
  $comparison = if ($IsWindows) { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }
  return $candidateFull.Equals($baseFull, $comparison) -or $candidateFull.StartsWith($baseWithSeparator, $comparison)
}

function ConvertTo-Iperf3HashtableFromObject {
  [CmdletBinding()]
  [OutputType([hashtable])]
  param(
    [AllowNull()]
    [object]$InputObject
  )
  if ($InputObject -is [hashtable]) { return $InputObject }
  if ($InputObject -is [System.Collections.IDictionary]) {
    $h = @{}
    foreach ($k in $InputObject.Keys) { $h[[string]$k] = $InputObject[$k] }
    return $h
  }
  $h = @{}
  if ($null -eq $InputObject) { return $h }
  foreach ($p in $InputObject.PSObject.Properties) {
    $h[$p.Name] = $p.Value
  }
  return $h
}
