<#
.SYNOPSIS
  Extended Windows network diagnostics with ICMP, traceroute, path loss, and TCP checks.

.DESCRIPTION
  Runs a test matrix of (rounds x protocols x hosts) using built-in Windows
  tools: ping, tracert, pathping, Test-NetConnection.

  Writes structured JSON and compact CSV for downstream analysis.
  Supports dry-run planning, protocol/round filtering, and quiet mode.

  Default hosts are loaded from config/hosts.conf when present.
  CLI parameter overrides take precedence over config file values.

.PARAMETER HostsIPv4
  Override IPv4 target hosts (default: from config/hosts.conf or built-in list).

.PARAMETER HostsIPv6
  Override IPv6 target hosts (default: from config/hosts.conf or built-in list).

.PARAMETER LogDirectory
  Output directory for JSON/CSV results (default: ~/logs).

.PARAMETER Protocols
  Filter by protocol family. Valid values: IPv4, IPv6 (default: both).

.PARAMETER Rounds
  Filter by round name (default: Standard). Use -ListRounds to see available rounds.

.PARAMETER SkipPathping
  Skip the pathping stage (pathping is slow; useful for quick runs).

.PARAMETER DryRun
  Print planned runs without executing any probes or creating output files.

.PARAMETER Quiet
  Suppress informational output; show only warnings, failures, and the final summary.

.PARAMETER ListRounds
  Print available round names and exit.

.PARAMETER ListProtocols
  Print available protocol families and exit.

.EXAMPLE
  .\apps\path\Test-NetworkPath.ps1 -DryRun
  Preview the bounded default test plan without running any diagnostics.

.EXAMPLE
  .\apps\path\Test-NetworkPath.ps1 -Protocols IPv4 -Rounds Standard -HostsIPv4 google.com
  Run only the Standard round over IPv4 against a single host.

.EXAMPLE
  .\apps\path\Test-NetworkPath.ps1 -SkipPathping -Quiet
  Full run without pathping, minimal console output.
#>

[CmdletBinding()]
param(
  [string[]]$HostsIPv4,
  [string[]]$HostsIPv6,

  [string]$LogDirectory = '',

  [int]$PingCount = 5,
  [int]$TraceMaxHops = 30,
  [int]$TraceTimeoutMs = 5000,
  [int]$PathpingProbes = 50,
  [int]$PathpingTimeoutMs = 3000,

  [ValidateSet('IPv4', 'IPv6')]
  [string[]]$Protocols = @('IPv4', 'IPv6'),

  [string[]]$Rounds = @('Standard'),

  [switch]$SkipPathping,
  [switch]$DryRun,
  [switch]$Quiet,
  [switch]$Version,
  [switch]$ListRounds,
  [switch]$ListProtocols
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($PSVersionTable.PSVersion.Major -ge 7) {
  $PSNativeCommandUseErrorActionPreference = $false
}

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$libPsRoot = Join-Path $repoRoot 'src/powershell/path/lib-ps'

# Source function files
Get-ChildItem -Path (Join-Path $libPsRoot '*.ps1') | Sort-Object Name | ForEach-Object { . $_.FullName }

$SuiteVersion = '1.1.0'

if ($Version) {
  Write-Output "Test-NetworkPath.ps1 v$SuiteVersion"
  return
}

if ($ListRounds) {
  @(Get-RoundDefinitions -TraceMaxHops $TraceMaxHops -TraceTimeoutMs $TraceTimeoutMs -PathpingProbes $PathpingProbes -PathpingTimeoutMs $PathpingTimeoutMs) | ForEach-Object { $_.Name }
  return
}

if ($ListProtocols) {
  Write-Output 'IPv4'
  Write-Output 'IPv6'
  return
}

$IsWindowsRuntime = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)
if (-not $IsWindowsRuntime -and -not $DryRun) {
  throw 'Full Network Lantern path diagnostics require Windows built-in tools (ping/tracert/pathping/Test-NetConnection). Use -DryRun on non-Windows hosts.'
}

$DefaultHostsConfig = Join-Path $repoRoot 'config/hosts.conf'

if ([string]::IsNullOrWhiteSpace($LogDirectory)) {
  $LogDirectory = Get-DefaultLogDirectory
}

if (-not (Test-PathSafe $LogDirectory)) {
  throw "LogDirectory must not be empty, start with '-', contain '|', control chars, or path traversal (..): $LogDirectory"
}

$defaultHosts4 = @('netcologne.de', 'google.com', 'wikipedia.org', 'amazon.de')
$defaultHosts6 = @('netcologne.de', 'google.com', 'wikipedia.org')

$configHosts = Get-HostsFromConfig -Path $DefaultHostsConfig

if (-not $PSBoundParameters.ContainsKey('HostsIPv4')) {
  $HostsIPv4 = if (@($configHosts.IPv4).Count -gt 0) { @($configHosts.IPv4) } else { @($defaultHosts4) }
}
if (-not $PSBoundParameters.ContainsKey('HostsIPv6')) {
  $HostsIPv6 = if (@($configHosts.IPv6).Count -gt 0) { @($configHosts.IPv6) } else { @($defaultHosts6) }
}

$badHosts = @(@($HostsIPv4) + @($HostsIPv6) | Where-Object { -not (Test-HostNameSafe $_) })
if (@($badHosts).Count -gt 0) {
  throw "Host names must not start with '-', contain whitespace, '/', '|', or control characters: $($badHosts -join ', ')"
}

$roundDefinitions = @(Get-RoundDefinitions -TraceMaxHops $TraceMaxHops -TraceTimeoutMs $TraceTimeoutMs -PathpingProbes $PathpingProbes -PathpingTimeoutMs $PathpingTimeoutMs)
if ($null -ne $Rounds) {
  $requestedRounds = @($Rounds | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
} else {
  $requestedRounds = @()
}
if (@($requestedRounds).Count -gt 0) {
  $selectedRoundDefs = @($roundDefinitions | Where-Object { $_.Name -in $requestedRounds })
  $missingRounds = @($requestedRounds | Where-Object { $_ -notin @($roundDefinitions.Name) })
  if (@($missingRounds).Count -gt 0) {
    throw "Unknown rounds: $($missingRounds -join ', '). Allowed: $($roundDefinitions.Name -join ', ')"
  }
  $roundDefinitions = $selectedRoundDefs
}

$selectedProtocols = @($Protocols | Select-Object -Unique)

# Optional service probes must be bound to their actual service endpoints. The
# generic path workflow therefore has no unrelated hard-coded port targets.
$portTargets = @()

$plan = Get-DiagnosticPlan -RoundDefinitions $roundDefinitions -SelectedProtocols $selectedProtocols -ResolvedHostsIPv4 @($HostsIPv4) -ResolvedHostsIPv6 @($HostsIPv6)
if ($plan.Count -eq 0) {
  throw 'No diagnostic runs planned. Check selected rounds/protocols/hosts.'
}

$ts = "$(Get-Date -Format 'yyyyMMdd_HHmmss')_$PID"
$jsonPath = Join-Path $LogDirectory "net_results_$ts.json"
$csvPath = Join-Path $LogDirectory "net_summary_$ts.csv"

Write-Status -Level INFO -Message "Planned runs: $($plan.Count)"
Write-Status -Level INFO -Message "Protocols: $($selectedProtocols -join ', ')"
Write-Status -Level INFO -Message "Rounds: $($roundDefinitions.Name -join ', ')"
Write-Status -Level INFO -Message "IPv4 hosts: $(@($HostsIPv4) -join ', ')"
Write-Status -Level INFO -Message "IPv6 hosts: $(@($HostsIPv6) -join ', ')"

if ($DryRun) {
  Write-Status -Level SUMMARY -Message "Dry-run only. Planned runs: $($plan.Count)"
  Write-Status -Level SUMMARY -Message "Would write JSON: $jsonPath"
  Write-Status -Level SUMMARY -Message "Would write CSV : $csvPath"
  if (-not $Quiet) {
    $i = 0
    foreach ($item in $plan) {
      $i++
      Write-Status -Level PLAN -Message "[$i/$($plan.Count)] round=$($item.RoundName) protocol=$($item.Protocol) host=$($item.Host)"
    }
  }
  return
}

New-Item -ItemType Directory -Path $LogDirectory -Force | Out-Null

$results = New-Object System.Collections.Generic.List[object]
$script:DiagnosticsCompletedNormally = $false
$start = Get-Date
$executionSettings = [pscustomobject]@{
  PingCount = $PingCount
  SkipPathping = [bool]$SkipPathping
}

Write-Status -Level INFO -Message "Starting diagnostics"

try {
  Invoke-DiagnosticsMatrix -Plan $plan -PortTargets $portTargets -Results $results -Settings $executionSettings
  Save-DiagnosticResults -Results @($results) -JsonPath $jsonPath -CsvPath $csvPath
  $script:DiagnosticsCompletedNormally = $true

  $elapsed = [int]((Get-Date) - $start).TotalSeconds
  $failedCount = @($results | Where-Object { $_.OverallStatus -eq 'Fail' }).Count
  $okCount = $results.Count - $failedCount
  Write-Status -Level SUMMARY -Message "Diagnostics complete. Passed: $okCount Failed: $failedCount Elapsed: ${elapsed}s"
}
finally {
  $resultsCount = $results.Count
  if ((-not [bool]$script:DiagnosticsCompletedNormally) -and ($resultsCount -gt 0)) {
    Write-Status -Level WARN -Message 'Interrupted or error: saving partial results.'
    try {
      Save-DiagnosticResults -Results @($results) -JsonPath $jsonPath -CsvPath $csvPath
    } catch {
      Write-Status -Level ERROR -Message "Failed to save partial results: $($_.Exception.Message)"
    }
  }
}

$anyFailure = $results | Where-Object { $_.OverallStatus -eq 'Fail' } | Select-Object -First 1
if ($anyFailure) {
  exit 1
}
