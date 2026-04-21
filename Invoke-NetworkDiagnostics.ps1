[CmdletBinding()]
param(
  [ValidateSet('Triage', 'Path', 'Throughput', 'Baseline', 'WindowsTuning')]
  [string]$Workflow = 'Triage',

  [string]$ProfilePath,

  [string[]]$HostsIPv4,

  [string[]]$HostsIPv6,

  [ValidateSet('IPv4', 'IPv6')]
  [string[]]$Protocols,

  [string[]]$Rounds,

  [string]$IperfTarget,

  [ValidateRange(1, 65535)]
  [int]$IperfPort = 5201,

  [ValidateSet('TCP', 'UDP', 'Both')]
  [string]$ThroughputProtocol = 'Both',

  [ValidateSet('Apply', 'Backup', 'Restore', 'Verify')]
  [string]$TuningAction = 'Apply',

  [ValidateSet('Safe', 'Measured')]
  [string]$TuningProfile = 'Safe',

  [uint16[]]$UdpPorts,

  [switch]$IncludeAppPolicies,

  [string[]]$AppPaths,

  [string]$OutRoot,

  [switch]$SkipPathping,

  [switch]$DryRun,

  [switch]$Quiet
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = $PSScriptRoot

function Resolve-ArtifactRoot {
  param([string]$Path)

  if ([string]::IsNullOrWhiteSpace($Path)) {
    return (Join-Path $repoRoot 'artifacts')
  }

  return [System.IO.Path]::GetFullPath($Path)
}

function Import-WorkflowProfile {
  param([string]$Path)

  if ([string]::IsNullOrWhiteSpace($Path)) {
    return @{}
  }

  $resolvedPath = if ([System.IO.Path]::IsPathRooted($Path)) {
    [System.IO.Path]::GetFullPath($Path)
  } else {
    [System.IO.Path]::GetFullPath((Join-Path (Get-Location) $Path))
  }
  if (-not (Test-Path -LiteralPath $resolvedPath -PathType Leaf)) {
    throw "ProfilePath is not a file: $resolvedPath"
  }

  $parsed = Get-Content -LiteralPath $resolvedPath -Raw -Encoding UTF8 | ConvertFrom-Json -AsHashtable
  if ($null -eq $parsed) {
    return @{}
  }

  return $parsed
}

function Get-ProfileSection {
  param(
    [hashtable]$SectionMap,
    [string]$Name
  )

  if ($SectionMap.ContainsKey($Name) -and $SectionMap[$Name] -is [hashtable]) {
    return $SectionMap[$Name]
  }

  return @{}
}

function Get-EffectiveValue {
  param(
    [object]$ExplicitValue,
    [hashtable]$Section,
    [string]$Key,
    [object]$Fallback = $null
  )

  if ($null -ne $ExplicitValue) {
    if ($ExplicitValue -is [array] -and $ExplicitValue.Count -eq 0) {
      if ($Section.ContainsKey($Key)) {
        return $Section[$Key]
      }

      return $Fallback
    }

    if ($ExplicitValue -is [string] -and [string]::IsNullOrWhiteSpace($ExplicitValue)) {
      if ($Section.ContainsKey($Key)) {
        return $Section[$Key]
      }

      return $Fallback
    }

    return $ExplicitValue
  }

  if ($Section.ContainsKey($Key)) {
    return $Section[$Key]
  }

  return $Fallback
}

$workflowProfile = Import-WorkflowProfile -Path $ProfilePath
$artifactRoot = Resolve-ArtifactRoot -Path $OutRoot
$pathSection = Get-ProfileSection -SectionMap $workflowProfile -Name 'path'
$throughputSection = Get-ProfileSection -SectionMap $workflowProfile -Name 'throughput'
$tuningSection = Get-ProfileSection -SectionMap $workflowProfile -Name 'windowsTuning'

$pathScript = Join-Path $repoRoot 'apps/path/NetPathSuite.ps1'
$throughputScript = Join-Path $repoRoot 'apps/throughput/iPerf3Test.ps1'
$tuningScript = Join-Path $repoRoot 'apps/windows-tuning/Optimize-NetworkPath.ps1'

$pathOutDir = Join-Path $artifactRoot 'path'
$throughputOutDir = Join-Path $artifactRoot 'throughput'

$effectiveHostsIPv4 = @(
  Get-EffectiveValue -ExplicitValue $HostsIPv4 -Section $pathSection -Key 'hostsIPv4' -Fallback @()
)
$effectiveHostsIPv6 = @(
  Get-EffectiveValue -ExplicitValue $HostsIPv6 -Section $pathSection -Key 'hostsIPv6' -Fallback @()
)
$effectiveProtocols = @(
  Get-EffectiveValue -ExplicitValue $Protocols -Section $pathSection -Key 'protocols' -Fallback @('IPv4', 'IPv6')
)
$effectiveRounds = @(
  Get-EffectiveValue -ExplicitValue $Rounds -Section $pathSection -Key 'rounds' -Fallback @()
)
$effectiveIperfTarget = Get-EffectiveValue -ExplicitValue $IperfTarget -Section $throughputSection -Key 'target'
$effectiveIperfPort = Get-EffectiveValue -ExplicitValue $IperfPort -Section $throughputSection -Key 'port' -Fallback 5201
$effectiveUdpPorts = @(
  Get-EffectiveValue -ExplicitValue $UdpPorts -Section $tuningSection -Key 'udpPorts' -Fallback @()
)
$effectiveAppPaths = @(
  Get-EffectiveValue -ExplicitValue $AppPaths -Section $tuningSection -Key 'appPaths' -Fallback @()
)
$effectiveThroughputProtocol = $ThroughputProtocol
$effectiveTuningAction = $TuningAction
$effectiveTuningProfile = $TuningProfile
$effectiveIncludeAppPolicies = [bool]$IncludeAppPolicies
$useSkipPathping = [bool]$SkipPathping
$useDryRun = [bool]$DryRun
$useQuiet = [bool]$Quiet

$isolatedPwshBootstrap = @'
$scriptPath = $env:NDS_CHILD_SCRIPT_PATH
$paramsJson = $env:NDS_CHILD_PARAMS_JSON

if ([string]::IsNullOrWhiteSpace($scriptPath)) {
  throw 'NDS_CHILD_SCRIPT_PATH is not set.'
}

$params = @{}
if (-not [string]::IsNullOrWhiteSpace($paramsJson)) {
  $params = ConvertFrom-Json -InputObject $paramsJson -AsHashtable
  foreach ($key in @($params.Keys)) {
    if ($params[$key] -is [System.Collections.IEnumerable] -and -not ($params[$key] -is [string])) {
      $params[$key] = @($params[$key])
    }
  }
}

& $scriptPath @params
exit $LASTEXITCODE
'@

$isolatedPwshBootstrapEncoded = [Convert]::ToBase64String([System.Text.Encoding]::Unicode.GetBytes($isolatedPwshBootstrap))

function Invoke-IsolatedPowerShellScript {
  param(
    [Parameter(Mandatory)][string]$ScriptPath,
    [Parameter(Mandatory)][hashtable]$Parameters
  )

  $previousScriptPath = $env:NDS_CHILD_SCRIPT_PATH
  $previousParamsJson = $env:NDS_CHILD_PARAMS_JSON
  $env:NDS_CHILD_SCRIPT_PATH = $ScriptPath
  $env:NDS_CHILD_PARAMS_JSON = ConvertTo-Json -InputObject $Parameters -Compress -Depth 8

  try {
    & pwsh -NoProfile -NonInteractive -OutputFormat Text -EncodedCommand $isolatedPwshBootstrapEncoded
  } finally {
    if ($null -eq $previousScriptPath) {
      Remove-Item Env:NDS_CHILD_SCRIPT_PATH -ErrorAction SilentlyContinue
    } else {
      $env:NDS_CHILD_SCRIPT_PATH = $previousScriptPath
    }

    if ($null -eq $previousParamsJson) {
      Remove-Item Env:NDS_CHILD_PARAMS_JSON -ErrorAction SilentlyContinue
    } else {
      $env:NDS_CHILD_PARAMS_JSON = $previousParamsJson
    }
  }
}

function Invoke-PathWorkflow {
  $scriptParams = @{
    LogDirectory = $pathOutDir
  }

  if ($effectiveProtocols.Count -gt 0) {
    $scriptParams['Protocols'] = $effectiveProtocols
  }
  if ($effectiveRounds.Count -gt 0) {
    $scriptParams['Rounds'] = $effectiveRounds
  }
  if ($effectiveHostsIPv4.Count -gt 0) {
    $scriptParams['HostsIPv4'] = $effectiveHostsIPv4
  }
  if ($effectiveHostsIPv6.Count -gt 0) {
    $scriptParams['HostsIPv6'] = $effectiveHostsIPv6
  }
  if ($useSkipPathping) { $scriptParams['SkipPathping'] = $true }
  if ($useDryRun) { $scriptParams['DryRun'] = $true }
  if ($useQuiet) { $scriptParams['Quiet'] = $true }

  Invoke-IsolatedPowerShellScript -ScriptPath $pathScript -Parameters $scriptParams
}

function Invoke-ThroughputWorkflow {
  if ([string]::IsNullOrWhiteSpace($effectiveIperfTarget)) {
    throw 'IperfTarget is required for Throughput or Baseline workflows.'
  }

  $scriptParams = @{
    Target = $effectiveIperfTarget
    Port = $effectiveIperfPort
    Protocol = $effectiveThroughputProtocol
    OutDir = $throughputOutDir
    ProfilesFile = (Join-Path $repoRoot 'profiles/throughput-profiles.json')
  }
  if ($Workflow -eq 'Baseline') { $scriptParams['SingleTest'] = $true }
  if ($useDryRun) { $scriptParams['WhatIf'] = $true }
  if ($useQuiet) { $scriptParams['Quiet'] = $true }

  Invoke-IsolatedPowerShellScript -ScriptPath $throughputScript -Parameters $scriptParams
}

function Invoke-TuningWorkflow {
  $scriptParams = @{
    Action = $effectiveTuningAction
    TuningProfile = $effectiveTuningProfile
  }
  if ($effectiveUdpPorts.Count -gt 0) {
    $scriptParams['UdpPorts'] = $effectiveUdpPorts
  }
  if ($effectiveIncludeAppPolicies) { $scriptParams['IncludeAppPolicies'] = $true }
  if ($effectiveAppPaths.Count -gt 0) {
    $scriptParams['AppPaths'] = $effectiveAppPaths
  }
  if ($useDryRun) { $scriptParams['DryRun'] = $true }

  Invoke-IsolatedPowerShellScript -ScriptPath $tuningScript -Parameters $scriptParams
}

switch ($Workflow) {
  'Path' {
    Invoke-PathWorkflow
    break
  }
  'Throughput' {
    Invoke-ThroughputWorkflow
    break
  }
  'Baseline' {
    Invoke-PathWorkflow
    Invoke-ThroughputWorkflow
    break
  }
  'WindowsTuning' {
    Invoke-TuningWorkflow
    break
  }
  'Triage' {
    Invoke-PathWorkflow
    if (-not [string]::IsNullOrWhiteSpace($effectiveIperfTarget)) {
      Invoke-ThroughputWorkflow
    }
    break
  }
}
