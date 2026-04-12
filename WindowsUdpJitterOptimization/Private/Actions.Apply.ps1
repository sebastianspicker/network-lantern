function Set-UjSystemResponsiveness {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]
    [ValidateSet(1, 2, 3)]
    [int]$Preset,

    [Parameter()]
    [switch]$DryRun
  )

  $value = switch ($Preset) {
    1 { 20 }
    2 { 10 }
    3 { 0 }
  }

  $mm = $script:UjRegistryPathSystemProfile
  if ($DryRun) {
    Write-UjInformation -Message ("[DryRun] SystemResponsiveness={0}" -f $value)
    return
  }

  if (-not $PSCmdlet.ShouldProcess($mm, ("Set SystemResponsiveness to {0}" -f $value))) {
    return
  }

  Set-UjRegistryValue -Key $mm -Name 'SystemResponsiveness' -Type DWord -Value $value -Confirm:$false
}

function Set-UjMmcssAudioTaskTuning {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([void])]
  param(
    [Parameter()]
    [switch]$DryRun
  )

  if ($DryRun) {
    Write-UjInformation -Message '[DryRun] Ensure MMCSS audio task registry values (experimental).'
    return
  }

  $mm = $script:UjRegistryPathSystemProfile
  $audio = Join-Path -Path (Join-Path -Path $mm -ChildPath 'Tasks') -ChildPath 'Audio'

  if (-not $PSCmdlet.ShouldProcess($audio, 'Set MMCSS audio task tuning values')) {
    return
  }

  Set-UjRegistryValue -Key $audio -Name 'Priority' -Type DWord -Value 6 -Confirm:$false
  Set-UjRegistryValue -Key $audio -Name 'BackgroundOnly' -Type DWord -Value 0 -Confirm:$false
  Set-UjRegistryValue -Key $audio -Name 'Clock Rate' -Type DWord -Value 10000 -Confirm:$false
  Set-UjRegistryValue -Key $audio -Name 'SchedulingCategory' -Type String -Value 'High' -Confirm:$false
  Set-UjRegistryValue -Key $audio -Name 'SFIOPriority' -Type String -Value 'High' -Confirm:$false
}

function Start-UjAudioService {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([void])]
  param(
    [Parameter()]
    [switch]$DryRun
  )

  if ($DryRun) {
    Write-UjInformation -Message '[DryRun] Ensure audio services are Automatic and running.'
    return
  }

  foreach ($serviceName in @('AudioEndpointBuilder', 'Audiosrv', 'MMCSS')) {
    try {
      if ($PSCmdlet.ShouldProcess($serviceName, 'Set service startup type to Automatic')) {
        Set-Service -Name $serviceName -StartupType Automatic
      }
    } catch {
      Write-Verbose -Message ("Set-Service failed: {0}" -f $serviceName)
    }

    try {
      $service = Get-Service -Name $serviceName
      if ($service.Status -ne 'Running' -and $PSCmdlet.ShouldProcess($serviceName, 'Start service')) {
        Start-Service -Name $serviceName
      }
    } catch {
      Write-Verbose -Message ("Start-Service failed: {0}" -f $serviceName)
    }
  }
}

function Enable-UjLocalQosMarking {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([void])]
  param(
    [Parameter()]
    [switch]$DryRun
  )

  if ($DryRun) {
    Write-UjInformation -Message '[DryRun] Enable local QoS marking (Do not use NLA).'
    return
  }

  $qos = $script:UjRegistryPathQos

  if (-not $PSCmdlet.ShouldProcess($qos, 'Enable local QoS marking (Do not use NLA)')) {
    return
  }

  Set-UjRegistryValue -Key $qos -Name 'Do not use NLA' -Type String -Value '1' -Confirm:$false
}

function Set-UjAfdFastSendDatagramThreshold {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]
    [ValidateSet(1, 2, 3)]
    [int]$Preset,

    [Parameter(Mandatory)]
    [ValidateRange(0, 65535)]
    [int]$AfdThreshold,

    [Parameter()]
    [switch]$DryRun
  )

  if ($Preset -lt 2) {
    return
  }

  $afd = $script:UjRegistryPathAfdParameters
  if ($DryRun) {
    Write-UjInformation -Message ("[DryRun] AFD FastSendDatagramThreshold={0}" -f $AfdThreshold)
    return
  }

  if (-not $PSCmdlet.ShouldProcess($afd, ("Set FastSendDatagramThreshold to {0}" -f $AfdThreshold))) {
    return
  }

  Set-UjRegistryValue -Key $afd -Name 'FastSendDatagramThreshold' -Type DWord -Value $AfdThreshold -Confirm:$false
  Write-UjInformation -Message ("AFD FastSendDatagramThreshold set to {0} (reboot recommended)." -f $AfdThreshold)
}

function Set-UjNetworkThrottlingIndex {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([void])]
  param(
    [Parameter()]
    [switch]$DryRun
  )

  $mm = $script:UjRegistryPathSystemProfile
  if ($DryRun) {
    Write-UjInformation -Message '[DryRun] NetworkThrottlingIndex=FFFFFFFF'
    return
  }

  if (-not $PSCmdlet.ShouldProcess($mm, 'Set NetworkThrottlingIndex=FFFFFFFF')) {
    return
  }

  Set-UjRegistryValue -Key $mm -Name 'NetworkThrottlingIndex' -Type DWord -Value 0xFFFFFFFF -Confirm:$false
}

function Set-UjUroState {
  [CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]
    [ValidateSet('Enabled', 'Disabled')]
    [string]$State,

    [Parameter()]
    [switch]$DryRun
  )

  $cmd = @('int', 'udp', 'set', 'global', ('uro={0}' -f $State.ToLowerInvariant()))
  if ($DryRun) {
    Write-UjInformation -Message ("[DryRun] netsh {0}" -f ($cmd -join ' '))
    return
  }

  if (-not $PSCmdlet.ShouldProcess('UDP', ("Set URO to {0}" -f $State))) {
    return
  }

  try {
    $null = & netsh @cmd 2>&1
    if ($LASTEXITCODE -ne 0) {
      Write-Warning -Message ("Could not change UDP Receive Offload (URO) setting. This feature may not be available on your version of Windows (netsh exit code {0})." -f $LASTEXITCODE)
    }
  } catch {
    Write-Warning -Message ("Could not change UDP Receive Offload (URO) setting: {0}" -f $_.Exception.Message)
  }
}

function Set-UjPowerPlan {
  [CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]
    [ValidateSet('Balanced', 'HighPerformance', 'Ultimate')]
    [string]$PowerPlan,

    [Parameter()]
    [switch]$DryRun
  )

  $guid =
    if ($PowerPlan -eq 'HighPerformance') { $script:UjPowerPlanGuidHighPerformance }
    elseif ($PowerPlan -eq 'Ultimate') { $script:UjPowerPlanGuidUltimate }
    else { $script:UjPowerPlanGuidBalanced }

  if ($DryRun) {
    Write-UjInformation -Message ("[DryRun] powercfg /S {0}" -f $guid)
    return
  }

  if (-not $PSCmdlet.ShouldProcess($PowerPlan, 'Set active power plan')) {
    return
  }

  if ($PowerPlan -eq 'Ultimate') {
    try {
      $dupOut = & powercfg /duplicatescheme $script:UjPowerPlanGuidUltimate 2>&1
      if ($LASTEXITCODE -eq 0 -and $dupOut) {
        $parsedGuid = Get-UjGuidFromText -Text ($dupOut -join ' ')
        if ($parsedGuid) { $guid = $parsedGuid }
      }
    } catch {
      Write-Verbose -Message 'powercfg /duplicatescheme failed.'
    }
  }

  $null = & powercfg /S $guid 2>&1
  if ($LASTEXITCODE -ne 0) {
    Write-Warning -Message ("Could not switch your power plan. The selected plan may not be available on this PC (error code {0})." -f $LASTEXITCODE)
  }
}

function Set-UjGameDvrState {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]
    [ValidateSet('Enabled', 'Disabled')]
    [string]$State,

    [Parameter()]
    [switch]$DryRun
  )

  if ($DryRun) {
    Write-UjInformation -Message ("[DryRun] Set GameDVR capture to {0}." -f $State)
    return
  }

  $dvr = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\GameDVR'
  if (-not (Test-Path -Path $dvr)) {
    return
  }

  $value = if ($State -eq 'Disabled') { 0 } else { 1 }
  if (-not $PSCmdlet.ShouldProcess($dvr, ("Set GameDVR capture to {0}" -f $State))) {
    return
  }

  New-ItemProperty -Path $dvr -Name 'AppCaptureEnabled' -PropertyType DWord -Value $value -Force | Out-Null
  New-ItemProperty -Path $dvr -Name 'HistoricalCaptureEnabled' -PropertyType DWord -Value $value -Force | Out-Null
}

function Show-UjSummary {
  [CmdletBinding()]
  [OutputType([void])]
  param()

  Write-UjInformation -Message "`n=== Performance Summary ==="
  Write-UjInformation -Message 'QoS Policies (Managed):'
  try {
    $managed = Get-UjManagedQosPolicy
    if ($managed) {
      Write-UjInformation -Message ($managed | Sort-Object -Property Name | Select-Object Name, DSCPAction, IPPortMatchCondition, AppPathNameMatchCondition | Format-Table -AutoSize | Out-String)
    } else {
      Write-UjInformation -Message '  No active managed QoS policies.'
    }
  } catch {
    Write-Verbose -Message 'QoS summary skipped.'
  }

  Write-UjInformation -Message "`nNIC Key Optimizations:"
  try {
    foreach ($nic in (Get-UjPhysicalUpAdapter)) {
      $props = Get-NetAdapterAdvancedProperty -Name $nic.Name |
        Where-Object { $_.DisplayName -match 'Energy|Interrupt|Flow|Offload|Large Send|Jumbo|Wake|Power|Green|NS|ARP|ITR|Buffer' }
      if ($props) {
        Write-UjInformation -Message ("  Adapter: {0}" -f $nic.Name)
        Write-UjInformation -Message ($props | Sort-Object -Property DisplayName | Select-Object DisplayName, DisplayValue | Format-Table -AutoSize | Out-String)
      }
    }
  } catch {
    Write-Verbose -Message 'NIC summary skipped.'
  }
}
