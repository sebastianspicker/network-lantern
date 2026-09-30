function New-NetworkLanternWorkflowPlan {
  [CmdletBinding()]
  param(
    [ValidateSet('Triage', 'Path', 'Throughput', 'Baseline', 'WindowsTuning')][string]$Workflow = 'Triage',
    [string]$ProfilePath,
    [string[]]$HostsIPv4,
    [string[]]$HostsIPv6,
    [ValidateSet('IPv4', 'IPv6')][string[]]$Protocols,
    [string[]]$Rounds,
    [string]$IperfTarget,
    [ValidateRange(1, 65535)][int]$IperfPort = 5201,
    [ValidateSet('TCP', 'UDP', 'Both')][string]$ThroughputProtocol = 'Both',
    [ValidateRange(0, 1000000)][int]$ThroughputMaxTotalTests = 0,
    [ValidateSet('Apply', 'Backup', 'Restore', 'Verify')][string]$TuningAction = 'Apply',
    [ValidateSet('Safe', 'Measured')][string]$TuningProfile = 'Safe',
    [uint16[]]$UdpPorts,
    [switch]$IncludeAppPolicies,
    [string[]]$AppPaths,
    [Parameter(Mandatory)][string]$OutRoot,
    [switch]$SkipPathping,
    [switch]$DryRun,
    [switch]$Quiet,
    [Parameter(Mandatory)][hashtable]$ExplicitParameters
  )

  $workflowProfile = Import-NetworkLanternWorkflowProfile -ProfilePath $ProfilePath
  Write-NetworkLanternWorkflowProfileWarnings -ProfileMap $workflowProfile
  $pathProfile = Get-NetworkLanternProfileSection -ProfileMap $workflowProfile -Name 'path'
  $throughputProfile = Get-NetworkLanternProfileSection -ProfileMap $workflowProfile -Name 'throughput'
  $tuningSection = Get-NetworkLanternProfileSection -ProfileMap $workflowProfile -Name 'windowsTuning'
  $artifacts = Get-NetworkLanternArtifactLocations -ArtifactRoot $OutRoot

  $effectiveHostsIPv4 = Resolve-NetworkLanternEffectiveValue -ExplicitValue $HostsIPv4 -ExplicitValueWasProvided $ExplicitParameters.ContainsKey('HostsIPv4') -Section $pathProfile -Key 'hostsIPv4' -Fallback @()
  $effectiveHostsIPv6 = Resolve-NetworkLanternEffectiveValue -ExplicitValue $HostsIPv6 -ExplicitValueWasProvided $ExplicitParameters.ContainsKey('HostsIPv6') -Section $pathProfile -Key 'hostsIPv6' -Fallback @()
  $effectiveProtocols = Resolve-NetworkLanternEffectiveValue -ExplicitValue $Protocols -ExplicitValueWasProvided $ExplicitParameters.ContainsKey('Protocols') -Section $pathProfile -Key 'protocols' -Fallback @('IPv4', 'IPv6')
  $effectiveRounds = Resolve-NetworkLanternEffectiveValue -ExplicitValue $Rounds -ExplicitValueWasProvided $ExplicitParameters.ContainsKey('Rounds') -Section $pathProfile -Key 'rounds' -Fallback @()
  $effectiveIperfTarget = Resolve-NetworkLanternEffectiveValue -ExplicitValue $IperfTarget -ExplicitValueWasProvided $ExplicitParameters.ContainsKey('IperfTarget') -Section $throughputProfile -Key 'target'
  $effectiveIperfPort = Resolve-NetworkLanternEffectiveValue -ExplicitValue $IperfPort -ExplicitValueWasProvided $ExplicitParameters.ContainsKey('IperfPort') -Section $throughputProfile -Key 'port' -Fallback 5201
  $effectiveThroughputProtocol = Resolve-NetworkLanternEffectiveValue -ExplicitValue $ThroughputProtocol -ExplicitValueWasProvided $ExplicitParameters.ContainsKey('ThroughputProtocol') -Section $throughputProfile -Key 'protocol' -Fallback 'Both'
  $effectiveThroughputMaxTotalTests = Resolve-NetworkLanternEffectiveValue -ExplicitValue $ThroughputMaxTotalTests -ExplicitValueWasProvided $ExplicitParameters.ContainsKey('ThroughputMaxTotalTests') -Section $throughputProfile -Key 'maxTotalTests' -Fallback 0
  $effectiveTuningAction = Resolve-NetworkLanternEffectiveValue -ExplicitValue $TuningAction -ExplicitValueWasProvided $ExplicitParameters.ContainsKey('TuningAction') -Section $tuningSection -Key 'action' -Fallback 'Apply'
  $effectiveTuningProfile = Resolve-NetworkLanternEffectiveValue -ExplicitValue $TuningProfile -ExplicitValueWasProvided $ExplicitParameters.ContainsKey('TuningProfile') -Section $tuningSection -Key 'profile' -Fallback 'Safe'
  $effectiveUdpPorts = Resolve-NetworkLanternEffectiveValue -ExplicitValue $UdpPorts -ExplicitValueWasProvided $ExplicitParameters.ContainsKey('UdpPorts') -Section $tuningSection -Key 'udpPorts' -Fallback @()
  $effectiveAppPaths = Resolve-NetworkLanternEffectiveValue -ExplicitValue $AppPaths -ExplicitValueWasProvided $ExplicitParameters.ContainsKey('AppPaths') -Section $tuningSection -Key 'appPaths' -Fallback @()

  $configuration = @{
    Workflow           = $Workflow
    HostsIPv4          = @(ConvertTo-NetworkLanternWorkflowStringArray -Value $effectiveHostsIPv4 -Name 'path.hostsIPv4')
    HostsIPv6          = @(ConvertTo-NetworkLanternWorkflowStringArray -Value $effectiveHostsIPv6 -Name 'path.hostsIPv6')
    Protocols          = @(ConvertTo-NetworkLanternWorkflowStringArray -Value $effectiveProtocols -Name 'path.protocols' -AllowedValues @('IPv4', 'IPv6'))
    Rounds             = @(ConvertTo-NetworkLanternWorkflowStringArray -Value $effectiveRounds -Name 'path.rounds')
    IperfTarget        = ConvertTo-NetworkLanternWorkflowString -Value $effectiveIperfTarget -Name 'throughput.target'
    IperfPort          = [int](ConvertTo-NetworkLanternWorkflowInteger -Value $effectiveIperfPort -Name 'throughput.port' -Minimum 1 -Maximum 65535)
    ThroughputProtocol = ConvertTo-NetworkLanternWorkflowString -Value $effectiveThroughputProtocol -Name 'throughput.protocol'
    ThroughputMaxTotalTests = [int](ConvertTo-NetworkLanternWorkflowInteger -Value $effectiveThroughputMaxTotalTests -Name 'throughput.maxTotalTests' -Minimum 0 -Maximum 1000000)
    TuningAction       = ConvertTo-NetworkLanternWorkflowString -Value $effectiveTuningAction -Name 'windowsTuning.action'
    TuningProfile      = ConvertTo-NetworkLanternWorkflowString -Value $effectiveTuningProfile -Name 'windowsTuning.profile'
    UdpPorts           = @(ConvertTo-NetworkLanternWorkflowUInt16Array -Value $effectiveUdpPorts -Name 'windowsTuning.udpPorts')
    AppPaths           = @(ConvertTo-NetworkLanternWorkflowStringArray -Value $effectiveAppPaths -Name 'windowsTuning.appPaths')
    IncludeAppPolicies = [bool]$IncludeAppPolicies
    SkipPathping       = [bool]$SkipPathping
    DryRun             = [bool]$DryRun
    Quiet              = [bool]$Quiet
  }

  if ($configuration.ThroughputProtocol -notin @('TCP', 'UDP', 'Both')) {
    throw "throughput.protocol contains unsupported value '$($configuration.ThroughputProtocol)'. Allowed values: TCP, UDP, Both."
  }
  if ($configuration.TuningAction -notin @('Apply', 'Backup', 'Restore', 'Verify')) {
    throw "windowsTuning.action contains unsupported value '$($configuration.TuningAction)'. Allowed values: Apply, Backup, Restore, Verify."
  }
  if ($configuration.TuningProfile -notin @('Safe', 'Measured')) {
    throw "windowsTuning.profile contains unsupported value '$($configuration.TuningProfile)'. Allowed values: Safe, Measured."
  }

  $steps = switch ($Workflow) {
    'Path' { @(New-NetworkLanternPathStep -Configuration $configuration -Artifacts $artifacts) }
    'Throughput' { @(New-NetworkLanternThroughputStep -Configuration $configuration -Artifacts $artifacts) }
    'Baseline' {
      @(
        (New-NetworkLanternPathStep -Configuration $configuration -Artifacts $artifacts)
        (New-NetworkLanternThroughputStep -Configuration $configuration -Artifacts $artifacts)
      )
    }
    'WindowsTuning' { @(New-NetworkLanternTuningStep -Configuration $configuration) }
    'Triage' {
      $triageSteps = @(New-NetworkLanternPathStep -Configuration $configuration -Artifacts $artifacts)
      if ([string]::IsNullOrWhiteSpace($configuration.IperfTarget)) {
        Write-Warning 'Throughput skipped: IperfTarget not provided for Triage workflow.'
      } else {
        $triageSteps += New-NetworkLanternThroughputStep -Configuration $configuration -Artifacts $artifacts
      }
      $triageSteps
    }
  }

  return [pscustomobject]@{
    Workflow = $Workflow
    Steps    = @($steps)
  }
}
