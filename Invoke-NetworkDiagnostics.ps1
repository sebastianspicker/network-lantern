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

  $resolvedPath = [System.IO.Path]::GetFullPath((Join-Path (Get-Location) $Path))
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

function Invoke-PathWorkflow {
  $commandArgs = @(
    '-File', $pathScript,
    '-LogDirectory', $pathOutDir
  )

  if ($effectiveProtocols.Count -gt 0) {
    $commandArgs += '-Protocols'
    $commandArgs += $effectiveProtocols
  }
  if ($effectiveRounds.Count -gt 0) {
    $commandArgs += '-Rounds'
    $commandArgs += $effectiveRounds
  }
  if ($effectiveHostsIPv4.Count -gt 0) {
    $commandArgs += '-HostsIPv4'
    $commandArgs += $effectiveHostsIPv4
  }
  if ($effectiveHostsIPv6.Count -gt 0) {
    $commandArgs += '-HostsIPv6'
    $commandArgs += $effectiveHostsIPv6
  }
  if ($useSkipPathping) { $commandArgs += '-SkipPathping' }
  if ($useDryRun) { $commandArgs += '-DryRun' }
  if ($useQuiet) { $commandArgs += '-Quiet' }

  & pwsh -NoProfile -NonInteractive @commandArgs
}

function Invoke-ThroughputWorkflow {
  if ([string]::IsNullOrWhiteSpace($effectiveIperfTarget)) {
    throw 'IperfTarget is required for Throughput or Baseline workflows.'
  }

  $commandArgs = @(
    '-File', $throughputScript,
    '-Target', $effectiveIperfTarget,
    '-Port', [string]$effectiveIperfPort,
    '-Protocol', $effectiveThroughputProtocol,
    '-OutDir', $throughputOutDir,
    '-ProfilesFile', (Join-Path $repoRoot 'profiles/throughput-profiles.json')
  )
  if ($Workflow -eq 'Baseline') { $commandArgs += '-SingleTest' }
  if ($useDryRun) { $commandArgs += '-WhatIf' }
  if ($useQuiet) { $commandArgs += '-Quiet' }

  & pwsh -NoProfile -NonInteractive @commandArgs
}

function Invoke-TuningWorkflow {
  $commandArgs = @(
    '-File', $tuningScript,
    '-Action', $effectiveTuningAction,
    '-TuningProfile', $effectiveTuningProfile
  )
  if ($effectiveUdpPorts.Count -gt 0) {
    $commandArgs += '-UdpPorts'
    $commandArgs += ($effectiveUdpPorts | ForEach-Object { [string]$_ })
  }
  if ($effectiveIncludeAppPolicies) { $commandArgs += '-IncludeAppPolicies' }
  if ($effectiveAppPaths.Count -gt 0) {
    $commandArgs += '-AppPaths'
    $commandArgs += $effectiveAppPaths
  }
  if ($useDryRun) { $commandArgs += '-DryRun' }

  & pwsh -NoProfile -NonInteractive @commandArgs
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
