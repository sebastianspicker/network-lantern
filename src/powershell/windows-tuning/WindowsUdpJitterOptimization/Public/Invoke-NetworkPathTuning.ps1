function Invoke-NetworkPathTuning {
  <#
  .SYNOPSIS
    Applies, backs up, restores, or verifies a conservative Windows tuning subset.

  .DESCRIPTION
    Exposes a reduced tuning surface for network-diagnostics-suite. The public contract keeps
    tuning optional, conservative, and reversible: QoS policy helpers, safe backup/restore,
    tier-1 NIC power-saving disables, and an optional high-performance power plan.
  #>
  [CmdletBinding()]
  param(
    [ValidateSet('Apply', 'Backup', 'Restore', 'Verify')]
    [string]$Action = 'Apply',

    [ValidateSet('Safe', 'Measured')]
    [string]$TuningProfile = 'Safe',

    [ValidateRange(0, 63)]
    [sbyte]$Dscp = 46,

    [uint16[]]$UdpPorts = @(),

    [switch]$IncludeAppPolicies,

    [string[]]$AppPaths = @(),

    [ValidateSet('None', 'HighPerformance')]
    [string]$PowerPlan = 'None',

    [string]$BackupFolder = $script:UjDefaultBackupFolder,

    [switch]$AllowUnsafeBackupFolder,

    [switch]$PassThru,

    [switch]$DryRun,

    [switch]$SkipAdminCheck
  )

  $isWindowsRuntime = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)
  if (-not $isWindowsRuntime -and -not $DryRun) {
    throw 'Windows tuning workflows require Windows. Use -DryRun for preview on non-Windows hosts.'
  }

  if (-not $SkipAdminCheck -and $Action -ne 'Verify') {
    Assert-UjAdministrator
  }

  if ($Action -in @('Apply', 'Backup', 'Restore') -and [string]::IsNullOrWhiteSpace($BackupFolder)) {
    throw 'BackupFolder must not be empty.'
  }

  if ($Action -in @('Apply', 'Backup', 'Restore') -and -not $AllowUnsafeBackupFolder -and (Test-UjUnsafeBackupFolder -Path $BackupFolder)) {
    throw 'BackupFolder appears unsafe because it points to a sensitive system directory. Use -AllowUnsafeBackupFolder to override intentionally.'
  }

  if (-not $DryRun -and $Action -in @('Apply', 'Backup', 'Restore')) {
    New-UjDirectory -Path $BackupFolder | Out-Null
  }

  $normalizedPorts = @($UdpPorts | Sort-Object -Unique)
  $normalizedApps = @($AppPaths | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
  $components = [ordered]@{}
  $warnings = [System.Collections.Generic.List[string]]::new()
  $success = $true

  if ($Action -eq 'Backup') {
    $backupResult = Backup-UjState -BackupFolder $BackupFolder -DryRun:$DryRun
    $backupStatus = Resolve-UjRestoreStatus -Result $backupResult -DefaultIfNull 'OK'
    $components['Backup'] = $backupStatus
    $success = $backupStatus -ne 'Warn'
  } elseif ($Action -eq 'Restore') {
    $restoreStatus = Restore-UjState -BackupFolder $BackupFolder -DryRun:$DryRun
    foreach ($name in $restoreStatus.Keys) {
      $components[$name] = $restoreStatus[$name]
      if ($restoreStatus[$name] -eq 'Warn') {
        $warnings.Add("Restore component '$name' completed with warning.") | Out-Null
        $success = $false
      }
    }
  } elseif ($Action -eq 'Verify') {
    $managedPolicies = @()
    $qosEnumerationAvailable = $true
    if (-not (Get-Command -Name Get-NetQosPolicy -ErrorAction SilentlyContinue)) {
      $warnings.Add('QoS verification is unavailable because Get-NetQosPolicy is not present on this platform.') | Out-Null
      $qosEnumerationAvailable = $false
      $success = $false
    }

    try {
      if ($qosEnumerationAvailable) {
        $managedPolicies = @(Get-UjManagedQosPolicy -ErrorOnFailure | Select-Object -ExpandProperty Name)
      }
    } catch {
      $warnings.Add("Could not enumerate managed QoS policies: $($_.Exception.Message)") | Out-Null
      $qosEnumerationAvailable = $false
      $success = $false
    }

    $localQosEnabled = $null
    try {
      $localQosEnabled = [string](Get-ItemProperty -Path $script:UjRegistryPathQos -Name 'Do not use NLA' -ErrorAction Stop).'Do not use NLA' -eq '1'
    } catch {
      $localQosEnabled = $null
    }

    $missingPorts = @()
    foreach ($port in $normalizedPorts) {
      $expectedName = "NDS_QOS_PORT_{0}" -f $port
      if ($expectedName -notin $managedPolicies) {
        $missingPorts += $port
      }
    }

    $components['LocalQos'] = if ($localQosEnabled -eq $true) { 'OK' } elseif ($null -eq $localQosEnabled) { 'Unknown' } else { 'Warn' }
    if (-not $qosEnumerationAvailable) {
      $components['QosPolicies'] = 'Unknown'
    } elseif ($missingPorts.Count -eq 0) {
      $components['QosPolicies'] = 'OK'
    } else {
      $components['QosPolicies'] = 'Warn'
    }

    if ($missingPorts.Count -gt 0) {
      $warnings.Add("Missing managed QoS port policies for: $($missingPorts -join ', ')") | Out-Null
      $success = $false
    }

    if ($components['LocalQos'] -ne 'OK' -or $components['QosPolicies'] -ne 'OK') {
      $success = $false
    }

    if (-not $PassThru) {
      return
    }

    return [pscustomobject]@{
      Action          = $Action
      TuningProfile   = $TuningProfile
      DryRun          = [bool]$DryRun
      Success         = [bool]$success
      BackupFolder    = $BackupFolder
      ManagedPolicies = $managedPolicies
      MissingPorts    = $missingPorts
      Components      = $components
      Warnings        = @($warnings)
      Timestamp       = Get-Date
    }
  } else {
    $backupResult = Backup-UjState -BackupFolder $BackupFolder -DryRun:$DryRun
    $backupStatus = Resolve-UjRestoreStatus -Result $backupResult -DefaultIfNull 'OK'
    $components['Backup'] = $backupStatus
    if ($backupStatus -eq 'Warn') {
      $warnings.Add('Backup completed with warnings: one or more components failed.') | Out-Null
      $success = $false
    }

    try {
      Enable-UjLocalQosMarking -DryRun:$DryRun
      $components['LocalQos'] = if ($DryRun) { 'Skipped' } else { 'OK' }
    } catch {
      $warnings.Add("LocalQos failed: $($_.Exception.Message)") | Out-Null
      $components['LocalQos'] = 'Warn'
      $success = $false
    }

    if ($normalizedPorts.Count -gt 0) {
      try {
        $qosPortPoliciesSucceeded = $true
        foreach ($port in $normalizedPorts) {
          $policyResult = New-UjDscpPolicyByPort -Name ("NDS_QOS_PORT_{0}" -f $port) -PortStart $port -PortEnd $port -Dscp $Dscp -DryRun:$DryRun
          if ($false -eq $policyResult) { $qosPortPoliciesSucceeded = $false }
        }
        $components['QosPortPolicies'] = if ($DryRun) { 'Skipped' } elseif ($qosPortPoliciesSucceeded) { 'OK' } else { 'Warn' }
        if (-not $DryRun -and -not $qosPortPoliciesSucceeded) {
          $warnings.Add('QosPortPolicies failed: one or more port policies could not be created.') | Out-Null
          $success = $false
        }
      } catch {
        $warnings.Add("QosPortPolicies failed: $($_.Exception.Message)") | Out-Null
        $components['QosPortPolicies'] = 'Warn'
        $success = $false
      }
    } else {
      $components['QosPortPolicies'] = 'Skipped'
    }

    if ($IncludeAppPolicies -and $normalizedApps.Count -gt 0) {
      try {
        $i = 0
        $qosAppPoliciesSucceeded = $true
        foreach ($path in $normalizedApps) {
          $i++
          $appPolicyResult = New-UjDscpPolicyByApp -Name ("NDS_QOS_APP_{0}" -f $i) -ExePath $path -Dscp $Dscp -DryRun:$DryRun
          if ($false -eq $appPolicyResult) { $qosAppPoliciesSucceeded = $false }
        }
        $components['QosAppPolicies'] = if ($DryRun) { 'Skipped' } elseif ($qosAppPoliciesSucceeded) { 'OK' } else { 'Warn' }
        if (-not $DryRun -and -not $qosAppPoliciesSucceeded) {
          $warnings.Add('QosAppPolicies failed: one or more app policies could not be created.') | Out-Null
          $success = $false
        }
      } catch {
        $warnings.Add("QosAppPolicies failed: $($_.Exception.Message)") | Out-Null
        $components['QosAppPolicies'] = 'Warn'
        $success = $false
      }
    } else {
      $components['QosAppPolicies'] = 'Skipped'
    }

    if ($TuningProfile -eq 'Measured') {
      try {
        $nicResult = Set-UjNicConfiguration -Preset 1 -DryRun:$DryRun
        $components['NicPowerSaving'] = if ($DryRun) { 'Skipped' } elseif ($false -eq $nicResult) { 'Warn' } else { 'OK' }
        if (-not $DryRun -and $false -eq $nicResult) {
          $warnings.Add('NicPowerSaving failed: one or more NIC settings could not be changed.') | Out-Null
          $success = $false
        }
      } catch {
        $warnings.Add("NicPowerSaving failed: $($_.Exception.Message)") | Out-Null
        $components['NicPowerSaving'] = 'Warn'
        $success = $false
      }
    } else {
      $components['NicPowerSaving'] = 'Skipped'
    }

    $effectivePowerPlan = if ($PowerPlan -eq 'None' -and $TuningProfile -eq 'Measured') { 'HighPerformance' } else { $PowerPlan }
    if ($effectivePowerPlan -ne 'None') {
      try {
        $powerPlanResult = Set-UjPowerPlan -PowerPlan $effectivePowerPlan -DryRun:$DryRun
        $components['PowerPlan'] = if ($DryRun) { 'Skipped' } elseif ($false -eq $powerPlanResult) { 'Warn' } else { 'OK' }
        if (-not $DryRun -and $false -eq $powerPlanResult) {
          $warnings.Add('PowerPlan failed: the selected power plan could not be activated.') | Out-Null
          $success = $false
        }
      } catch {
        $warnings.Add("PowerPlan failed: $($_.Exception.Message)") | Out-Null
        $components['PowerPlan'] = 'Warn'
        $success = $false
      }
    } else {
      $components['PowerPlan'] = 'Skipped'
    }
  }

  if (-not $PassThru) {
    return
  }

  return [pscustomobject]@{
    Action              = $Action
    TuningProfile       = if ($Action -eq 'Apply') { $TuningProfile } else { $null }
    Dscp                = $Dscp
    UdpPorts            = $normalizedPorts
    IncludeAppPolicies  = [bool]$IncludeAppPolicies
    AppPaths            = $normalizedApps
    DryRun              = [bool]$DryRun
    Success             = [bool]$success
    BackupFolder        = if ($Action -in @('Apply', 'Backup', 'Restore')) { $BackupFolder } else { $null }
    Components          = $components
    Warnings            = @($warnings)
    Timestamp           = Get-Date
  }
}
