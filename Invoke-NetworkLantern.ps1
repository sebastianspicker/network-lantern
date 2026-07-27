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
$script:MaxWorkflowProfileBytes = 1MB

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
  $profileInfo = Get-Item -LiteralPath $resolvedPath
  if ($profileInfo.Length -gt $script:MaxWorkflowProfileBytes) {
    throw "ProfilePath exceeds maximum size (1 MB): $resolvedPath"
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

function Write-WorkflowProfileWarnings {
  param([hashtable]$ProfileMap)

  $knownKeysBySection = @{
    path          = @('hostsIPv4', 'hostsIPv6', 'protocols', 'rounds')
    throughput    = @('target', 'port', 'protocol')
    windowsTuning = @('action', 'profile', 'udpPorts', 'appPaths')
  }

  foreach ($sectionName in @($ProfileMap.Keys)) {
    if (-not $knownKeysBySection.ContainsKey($sectionName)) {
      Write-Warning -Message "Unknown workflow profile section '$sectionName' will be ignored."
      continue
    }

    $section = $ProfileMap[$sectionName]
    if ($section -isnot [hashtable]) {
      Write-Warning -Message "Workflow profile section '$sectionName' is not an object and will be ignored."
      continue
    }

    $knownKeys = @($knownKeysBySection[$sectionName])
    foreach ($key in @($section.Keys)) {
      if ($key -notin $knownKeys) {
        Write-Warning -Message "Unknown workflow profile key '$sectionName.$key' will be ignored."
      }
    }
  }
}

function Get-EffectiveValue {
  param(
    [object]$ExplicitValue,
    [bool]$ExplicitValueWasProvided = ($null -ne $ExplicitValue),
    [hashtable]$Section,
    [string]$Key,
    [object]$Fallback = $null
  )

  if ($ExplicitValueWasProvided -and $null -ne $ExplicitValue) {
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
Write-WorkflowProfileWarnings -ProfileMap $workflowProfile
$artifactRoot = Resolve-ArtifactRoot -Path $OutRoot
$pathSection = Get-ProfileSection -SectionMap $workflowProfile -Name 'path'
$throughputSection = Get-ProfileSection -SectionMap $workflowProfile -Name 'throughput'
$tuningSection = Get-ProfileSection -SectionMap $workflowProfile -Name 'windowsTuning'

$pathScript = Join-Path $repoRoot 'apps/path/Test-NetworkPath.ps1'
$throughputScript = Join-Path $repoRoot 'apps/throughput/Measure-NetworkThroughput.ps1'
$tuningScript = Join-Path $repoRoot 'apps/windows-tuning/Invoke-NetworkPathTuning.ps1'

$pathOutDir = Join-Path $artifactRoot 'path'
$throughputOutDir = Join-Path $artifactRoot 'throughput'

$effectiveHostsIPv4 = @(
  Get-EffectiveValue -ExplicitValue $HostsIPv4 -ExplicitValueWasProvided $PSBoundParameters.ContainsKey('HostsIPv4') -Section $pathSection -Key 'hostsIPv4' -Fallback @()
)
$effectiveHostsIPv6 = @(
  Get-EffectiveValue -ExplicitValue $HostsIPv6 -ExplicitValueWasProvided $PSBoundParameters.ContainsKey('HostsIPv6') -Section $pathSection -Key 'hostsIPv6' -Fallback @()
)
$effectiveProtocols = @(
  Get-EffectiveValue -ExplicitValue $Protocols -ExplicitValueWasProvided $PSBoundParameters.ContainsKey('Protocols') -Section $pathSection -Key 'protocols' -Fallback @('IPv4', 'IPv6')
)
$effectiveRounds = @(
  Get-EffectiveValue -ExplicitValue $Rounds -ExplicitValueWasProvided $PSBoundParameters.ContainsKey('Rounds') -Section $pathSection -Key 'rounds' -Fallback @()
)
$effectiveIperfTarget = Get-EffectiveValue -ExplicitValue $IperfTarget -ExplicitValueWasProvided $PSBoundParameters.ContainsKey('IperfTarget') -Section $throughputSection -Key 'target'
$effectiveIperfPort = Get-EffectiveValue -ExplicitValue $IperfPort -ExplicitValueWasProvided $PSBoundParameters.ContainsKey('IperfPort') -Section $throughputSection -Key 'port' -Fallback 5201
$effectiveUdpPorts = @(
  Get-EffectiveValue -ExplicitValue $UdpPorts -ExplicitValueWasProvided $PSBoundParameters.ContainsKey('UdpPorts') -Section $tuningSection -Key 'udpPorts' -Fallback @()
)
$effectiveAppPaths = @(
  Get-EffectiveValue -ExplicitValue $AppPaths -ExplicitValueWasProvided $PSBoundParameters.ContainsKey('AppPaths') -Section $tuningSection -Key 'appPaths' -Fallback @()
)
$effectiveThroughputProtocol = Get-EffectiveValue -ExplicitValue $ThroughputProtocol -ExplicitValueWasProvided $PSBoundParameters.ContainsKey('ThroughputProtocol') -Section $throughputSection -Key 'protocol' -Fallback 'Both'
$effectiveTuningAction = Get-EffectiveValue -ExplicitValue $TuningAction -ExplicitValueWasProvided $PSBoundParameters.ContainsKey('TuningAction') -Section $tuningSection -Key 'action' -Fallback 'Apply'
$effectiveTuningProfile = Get-EffectiveValue -ExplicitValue $TuningProfile -ExplicitValueWasProvided $PSBoundParameters.ContainsKey('TuningProfile') -Section $tuningSection -Key 'profile' -Fallback 'Safe'
$effectiveIncludeAppPolicies = [bool]$IncludeAppPolicies
$useSkipPathping = [bool]$SkipPathping
$useDryRun = [bool]$DryRun
$useQuiet = [bool]$Quiet

$isolatedPwshBootstrap = @'
$scriptPath = $env:NETWORK_LANTERN_CHILD_SCRIPT_PATH
$paramsJson = $env:NETWORK_LANTERN_CHILD_PARAMS_JSON

if ([string]::IsNullOrWhiteSpace($scriptPath)) {
  throw 'NETWORK_LANTERN_CHILD_SCRIPT_PATH is not set.'
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

  $previousScriptPath = $env:NETWORK_LANTERN_CHILD_SCRIPT_PATH
  $previousParamsJson = $env:NETWORK_LANTERN_CHILD_PARAMS_JSON
  $env:NETWORK_LANTERN_CHILD_SCRIPT_PATH = $ScriptPath
  $env:NETWORK_LANTERN_CHILD_PARAMS_JSON = ConvertTo-Json -InputObject $Parameters -Compress -Depth 8

  try {
    & pwsh -NoProfile -NonInteractive -OutputFormat Text -EncodedCommand $isolatedPwshBootstrapEncoded
    if ($LASTEXITCODE -ne 0) {
      # The child runs in a separate PowerShell process so its failure cannot
      # terminate this wrapper implicitly. Preserve its exact status instead.
      exit $LASTEXITCODE
    }
  } finally {
    if ($null -eq $previousScriptPath) {
      Remove-Item Env:NETWORK_LANTERN_CHILD_SCRIPT_PATH -ErrorAction SilentlyContinue
    } else {
      $env:NETWORK_LANTERN_CHILD_SCRIPT_PATH = $previousScriptPath
    }

    if ($null -eq $previousParamsJson) {
      Remove-Item Env:NETWORK_LANTERN_CHILD_PARAMS_JSON -ErrorAction SilentlyContinue
    } else {
      $env:NETWORK_LANTERN_CHILD_PARAMS_JSON = $previousParamsJson
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
    ProfilesFile = (Join-Path $repoRoot 'profiles/throughput-profiles.local.json')
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
    } else {
      Write-Warning 'Throughput skipped: IperfTarget not provided for Triage workflow.'
    }
    break
  }
}
