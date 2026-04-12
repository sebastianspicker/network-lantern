function Invoke-UdpJitterOptimization {
  <#
  .SYNOPSIS
    Applies, backs up, restores, or resets UDP jitter optimization settings on Windows 10/11.

  .DESCRIPTION
    Applies preset-based UDP jitter optimizations (QoS DSCP, NIC tuning, AFD, MMCSS, URO, power plan, Game DVR),
    or backs up/restores state, or resets to baseline. Requires elevation unless -SkipAdminCheck is used.

    Presets apply evidence-based settings:
    - Preset 1 (Safe): QoS priority tagging for your game/voice ports, turns off NIC power-saving
      features that add latency. Zero risk, no tradeoffs.
    - Preset 2 (Moderate): Adds interrupt and flow control tuning for faster packet processing.
      Slightly higher CPU usage (+1-5%).
    - Preset 3 (Aggressive): Maximum jitter reduction. Disables UDP batching and checksum offload.
      Noticeably higher CPU usage.

    Use -IncludeExperimental for TCP-only, Wake-on-LAN, and unproven settings (not recommended unless testing).

  .PARAMETER Action
    Apply, Backup, Restore, or ResetDefaults.

  .PARAMETER Preset
    Risk level 1 (Safe), 2 (Moderate), 3 (Aggressive). Used when Action is Apply.

  .PARAMETER BackupFolder
    Directory for backup/restore files. Default: ProgramData\UDPTune.

  .PARAMETER AllowUnsafeBackupFolder
    Allow backup/restore paths under sensitive system directories.

  .PARAMETER PassThru
    Return a structured result object containing action metadata and component status.

  .PARAMETER DryRun
    Print what would be done without making changes.

  .PARAMETER SkipAdminCheck
    Skip administrator privilege check.

  .EXAMPLE
    Invoke-UdpJitterOptimization -Action Apply -Preset 2 -WhatIf

  .EXAMPLE
    Invoke-UdpJitterOptimization -Action Backup -BackupFolder C:\MyBackup -PassThru
  #>
  [CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
  param(
    [Parameter()]
    [ValidateSet('Apply', 'Backup', 'Restore', 'ResetDefaults')]
    [string]$Action = 'Apply',

    [Parameter()]
    [ValidateSet(1, 2, 3)]
    [int]$Preset = 1,

    [Parameter()]
    [ValidateRange(1, 65535)]
    [int]$TeamSpeakPort = 9987,

    [Parameter()]
    [ValidateRange(1, 65535)]
    [int]$CS2PortStart = 27015,

    [Parameter()]
    [ValidateRange(1, 65535)]
    [int]$CS2PortEnd = 27036,

    [Parameter()]
    [switch]$IncludeAppPolicies,

    [Parameter()]
    [string[]]$AppPaths = @(),

    [Parameter()]
    [ValidateRange(0, 65535)]
    [int]$AfdThreshold = 1500,

    [Parameter()]
    [ValidateSet('None', 'HighPerformance', 'Ultimate')]
    [string]$PowerPlan = 'None',

    [Parameter()]
    [switch]$DisableGameDvr,

    [Parameter()]
    [switch]$DisableUro,

    [Parameter()]
    [switch]$IncludeExperimental,

    [Parameter()]
    [string]$BackupFolder = $script:UjDefaultBackupFolder,

    [Parameter()]
    [switch]$AllowUnsafeBackupFolder,

    [Parameter()]
    [switch]$PassThru,

    [Parameter()]
    [switch]$DryRun,

    [Parameter()]
    [switch]$SkipAdminCheck
  )

  $target = if ($env:COMPUTERNAME) { $env:COMPUTERNAME } else { 'LocalMachine' }
  $shouldProcessAction =
    if ($Action -eq 'Backup') { 'Backup UDP jitter optimization state' }
    elseif ($Action -eq 'Restore') { 'Restore UDP jitter optimization state' }
    elseif ($Action -eq 'ResetDefaults') { 'Reset UDP jitter optimization settings to baseline defaults' }
    else { "Apply UDP jitter optimization preset $Preset" }

  if (-not $PSCmdlet.ShouldProcess($target, $shouldProcessAction)) {
    return
  }

  if (-not $SkipAdminCheck) {
    Assert-UjAdministrator
  }

  if ($Action -eq 'Apply' -and $CS2PortEnd -lt $CS2PortStart) {
    throw 'CS2PortEnd must be greater than or equal to CS2PortStart.'
  }

  if ($Action -eq 'Apply' -and $TeamSpeakPort -ge $CS2PortStart -and $TeamSpeakPort -le $CS2PortEnd) {
    Write-Warning -Message ("Your TeamSpeak port ({0}) overlaps with your CS2 port range ({1}-{2}). Both will still get priority, but the rules may interfere with each other." -f $TeamSpeakPort, $CS2PortStart, $CS2PortEnd)
  }

  if ($Action -in @('Backup', 'Restore', 'Apply') -and [string]::IsNullOrWhiteSpace($BackupFolder)) {
    throw 'BackupFolder must not be empty.'
  }

  if ($Action -in @('Backup', 'Restore', 'Apply') -and -not $AllowUnsafeBackupFolder -and (Test-UjUnsafeBackupFolder -Path $BackupFolder)) {
    throw 'BackupFolder appears unsafe because it points to a sensitive system directory. Use -AllowUnsafeBackupFolder to override intentionally.'
  }

  if (-not $DryRun -and $Action -in @('Backup', 'Restore', 'Apply')) {
    New-UjDirectory -Path $BackupFolder | Out-Null
  }

  $warnings = [System.Collections.Generic.List[string]]::new()
  $components = [ordered]@{}
  $success = $true

  if ($Action -eq 'Backup') {
    $backupResult = Backup-UjState -BackupFolder $BackupFolder -DryRun:$DryRun
    $backupStatus = Resolve-UjRestoreStatus -Result $backupResult -DefaultIfNull 'OK'
    $components['Backup'] = $backupStatus
    if ($backupStatus -eq 'Warn') {
      $success = $false
      $warnings.Add('Backup completed with warnings: one or more components failed.') | Out-Null
    }
    Write-UjInformation -Message 'Backup complete.'
  } elseif ($Action -eq 'Restore') {
    $restoreStatus = Restore-UjState -BackupFolder $BackupFolder -DryRun:$DryRun
    foreach ($name in $restoreStatus.Keys) {
      $components[$name] = $restoreStatus[$name]
      if ($restoreStatus[$name] -eq 'Warn') {
        $success = $false
        $warnings.Add("Restore component '$name' completed with warning.") | Out-Null
      }
    }
    Write-UjInformation -Message 'Restore complete. A reboot may be required.'
  } elseif ($Action -eq 'ResetDefaults') {
    Reset-UjBaseline -DryRun:$DryRun
    $components['Reset'] = if ($DryRun) { 'Skipped' } else { 'OK' }
  } else {
    Write-UjInformation -Message ("UDP Jitter Optimization - Preset {0} (Action={1}){2}" -f $Preset, $Action, $(if ($IncludeExperimental) { ' [+Experimental]' } else { '' }))

    $backupResult = Backup-UjState -BackupFolder $BackupFolder -DryRun:$DryRun
    $backupStatus = Resolve-UjRestoreStatus -Result $backupResult -DefaultIfNull 'OK'
    $components['Backup'] = $backupStatus
    if ($backupStatus -eq 'Warn') {
      $success = $false
      $warnings.Add('Backup completed with warnings: one or more components failed.') | Out-Null
    }

    # MMCSS service (enabler for NetworkThrottlingIndex and SystemResponsiveness)
    try {
      Start-UjAudioService -DryRun:$DryRun
      $components['AudioServices'] = if ($DryRun) { 'Skipped' } else { 'OK' }
    } catch {
      $components['AudioServices'] = 'Warn'
      $success = $false
      $warnings.Add("AudioServices failed: $($_.Exception.Message)") | Out-Null
    }

    # SystemResponsiveness: value varies by preset (20/10/0)
    try {
      Set-UjSystemResponsiveness -Preset $Preset -DryRun:$DryRun
      $components['SystemResponsiveness'] = if ($DryRun) { 'Skipped' } else { 'OK' }
    } catch {
      $components['SystemResponsiveness'] = 'Warn'
      $success = $false
      $warnings.Add("SystemResponsiveness failed: $($_.Exception.Message)") | Out-Null
    }

    try {
      Enable-UjLocalQosMarking -DryRun:$DryRun
      $components['LocalQos'] = if ($DryRun) { 'Skipped' } else { 'OK' }
    } catch {
      $components['LocalQos'] = 'Warn'
      $success = $false
      $warnings.Add("LocalQos failed: $($_.Exception.Message)") | Out-Null
    }

    try {
      New-UjDscpPolicyByPort -Name ("QoS_UDP_TS_{0}" -f $TeamSpeakPort) -PortStart $TeamSpeakPort -PortEnd $TeamSpeakPort -Dscp $script:UjDefaultDscp -DryRun:$DryRun
      New-UjDscpPolicyByPort -Name ("QoS_UDP_CS2_{0}_{1}" -f $CS2PortStart, $CS2PortEnd) -PortStart $CS2PortStart -PortEnd $CS2PortEnd -Dscp $script:UjDefaultDscp -DryRun:$DryRun
      $components['QosPortPolicies'] = if ($DryRun) { 'Skipped' } else { 'OK' }
    } catch {
      $components['QosPortPolicies'] = 'Warn'
      $success = $false
      $warnings.Add("QosPortPolicies failed: $($_.Exception.Message)") | Out-Null
    }

    if ($IncludeAppPolicies -and $null -ne $AppPaths -and $AppPaths.Count -gt 0) {
      try {
        $i = 0
        foreach ($path in $AppPaths) {
          if ([string]::IsNullOrWhiteSpace($path)) {
            continue
          }
          $i++
          New-UjDscpPolicyByApp -Name ('QoS_APP_{0}' -f $i) -ExePath $path -Dscp $script:UjDefaultDscp -DryRun:$DryRun
        }
        $components['QosAppPolicies'] = if ($DryRun) { 'Skipped' } else { 'OK' }
      } catch {
        $components['QosAppPolicies'] = 'Warn'
        $success = $false
        $warnings.Add("QosAppPolicies failed: $($_.Exception.Message)") | Out-Null
      }
    } else {
      $components['QosAppPolicies'] = 'Skipped'
    }

    try {
      Set-UjNicConfiguration -Preset $Preset -IncludeExperimental:$IncludeExperimental -DryRun:$DryRun
      $components['Nic'] = if ($DryRun) { 'Skipped' } else { 'OK' }
    } catch {
      $components['Nic'] = 'Warn'
      $success = $false
      $warnings.Add("Nic configuration failed: $($_.Exception.Message)") | Out-Null
    }

    try {
      Set-UjAfdFastSendDatagramThreshold -Preset $Preset -AfdThreshold $AfdThreshold -DryRun:$DryRun
      $components['Afd'] = if ($Preset -lt 2) { 'Skipped' } elseif ($DryRun) { 'Skipped' } else { 'OK' }
    } catch {
      $components['Afd'] = 'Warn'
      $success = $false
      $warnings.Add("Afd failed: $($_.Exception.Message)") | Out-Null
    }

    # NetworkThrottlingIndex: Preset 2+ (well-documented MMCSS setting)
    if ($Preset -ge 2) {
      try {
        Set-UjNetworkThrottlingIndex -DryRun:$DryRun
        $components['NetworkThrottlingIndex'] = if ($DryRun) { 'Skipped' } else { 'OK' }
      } catch {
        $components['NetworkThrottlingIndex'] = 'Warn'
        $success = $false
        $warnings.Add("NetworkThrottlingIndex failed: $($_.Exception.Message)") | Out-Null
      }
    } else {
      $components['NetworkThrottlingIndex'] = 'Skipped'
    }

    if ($DisableUro -or $Preset -ge 3) {
      try {
        Set-UjUroState -State Disabled -DryRun:$DryRun
        $components['Uro'] = if ($DryRun) { 'Skipped' } else { 'OK' }
      } catch {
        $components['Uro'] = 'Warn'
        $success = $false
        $warnings.Add("Uro failed: $($_.Exception.Message)") | Out-Null
      }
    } else {
      $components['Uro'] = 'Skipped'
    }

    if ($PowerPlan -ne 'None') {
      try {
        Set-UjPowerPlan -PowerPlan $PowerPlan -DryRun:$DryRun
        $components['PowerPlan'] = if ($DryRun) { 'Skipped' } else { 'OK' }
      } catch {
        $components['PowerPlan'] = 'Warn'
        $success = $false
        $warnings.Add("PowerPlan failed: $($_.Exception.Message)") | Out-Null
      }
    } else {
      $components['PowerPlan'] = 'Skipped'
    }

    # Experimental: MMCSS Audio Task tuning (audio scheduling, not UDP network)
    if ($IncludeExperimental) {
      try {
        Set-UjMmcssAudioTaskTuning -DryRun:$DryRun
        $components['MmcssAudioTaskTuning'] = if ($DryRun) { 'Skipped' } else { 'OK' }
      } catch {
        $components['MmcssAudioTaskTuning'] = 'Warn'
        $success = $false
        $warnings.Add("MmcssAudioTaskTuning failed: $($_.Exception.Message)") | Out-Null
      }
    } else {
      $components['MmcssAudioTaskTuning'] = 'Skipped'
    }

    if ($DisableGameDvr) {
      try {
        Set-UjGameDvrState -State Disabled -DryRun:$DryRun
        $components['GameDvr'] = if ($DryRun) { 'Skipped' } else { 'OK' }
      } catch {
        $components['GameDvr'] = 'Warn'
        $success = $false
        $warnings.Add("GameDvr failed: $($_.Exception.Message)") | Out-Null
      }
    } else {
      $components['GameDvr'] = 'Skipped'
    }

    Show-UjSummary
    Write-UjInformation -Message 'Note: Reboot recommended for AFD/MMCSS registry changes to fully apply.'
  }

  if (-not $PassThru) {
    return
  }

  return [pscustomobject]@{
    Action              = $Action
    Preset              = if ($Action -eq 'Apply') { $Preset } else { $null }
    IncludeExperimental = if ($Action -eq 'Apply') { [bool]$IncludeExperimental } else { $null }
    DryRun              = [bool]$DryRun
    Success             = [bool]$success
    BackupFolder        = if ($Action -in @('Backup', 'Restore', 'Apply')) { $BackupFolder } else { $null }
    Timestamp           = (Get-Date)
    Components          = $components
    Warnings            = @($warnings)
  }
}
