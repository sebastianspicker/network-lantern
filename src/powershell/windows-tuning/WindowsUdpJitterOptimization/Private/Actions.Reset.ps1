function Reset-UjBaseline {
  [CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
  [OutputType([void])]
  param(
    [Parameter()]
    [switch]$DryRun
  )

  Write-UjInformation -Message 'Resetting all settings to Windows baseline defaults...'

  Set-UjPowerPlan -PowerPlan Balanced -DryRun:$DryRun

  $mmKey = $script:UjRegistryPathSystemProfile
  $afdKey = $script:UjRegistryPathAfdParameters
  $gamesKey = Join-Path -Path $mmKey -ChildPath 'Tasks\Games'
  $audioKey = Join-Path -Path $mmKey -ChildPath 'Tasks\Audio'
  $qosRegKey = $script:UjRegistryPathQos

  if (-not $DryRun -and $PSCmdlet.ShouldProcess($mmKey, 'Remove registry tweaks')) {
    $tweaks = @(
      @{ Key = $mmKey; Name = 'NetworkThrottlingIndex' },
      @{ Key = $mmKey; Name = 'SystemResponsiveness' },
      @{ Key = $afdKey; Name = 'FastSendDatagramThreshold' }
    )
    foreach ($tweak in $tweaks) {
      if (Test-Path -Path $tweak.Key) {
        Remove-ItemProperty -Path $tweak.Key -Name $tweak.Name -ErrorAction SilentlyContinue
      }
    }
    if (Test-Path -Path $gamesKey) {
      Remove-Item -Path $gamesKey -Recurse -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path -Path $audioKey) {
      Remove-Item -Path $audioKey -Recurse -Force -ErrorAction SilentlyContinue
    }
  } elseif ($DryRun) {
    Write-UjInformation -Message '[DryRun] Remove registry tweaks (Throttling/Responsiveness/AFD/MMCSS Games/Audio)'
  }

  Set-UjGameDvrState -State Enabled -DryRun:$DryRun

  if ($DryRun) {
    Write-UjInformation -Message '[DryRun] Reset NIC properties to driver defaults.'
  } else {
    try {
      $adapters = Get-UjPhysicalUpAdapter
      foreach ($adapter in $adapters) {
        Write-UjInformation -Message ("  Resetting {0} ..." -f $adapter.Name)
        foreach ($keyword in $script:UjNicResetKeywords) {
          if ($PSCmdlet.ShouldProcess(("{0}: {1}" -f $adapter.Name, $keyword), 'Reset NetAdapterAdvancedProperty')) {
            try {
              Reset-NetAdapterAdvancedProperty -Name $adapter.Name -RegistryKeyword $keyword -Confirm:$false -ErrorAction Stop | Out-Null
            } catch {
              Write-Verbose -Message ("Reset property '{0}' on {1}: {2}" -f $keyword, $adapter.Name, $_.Exception.Message)
            }
          }
        }
        if ($PSCmdlet.ShouldProcess($adapter.Name, 'Enable NetAdapterRsc')) {
          Enable-NetAdapterRsc -Name $adapter.Name -ErrorAction SilentlyContinue | Out-Null
        }
      }
    } catch {
      Write-Warning -Message ("Could not reset network adapter settings to defaults: {0}" -f $_.Exception.Message)
    }
  }

  if ($DryRun) {
    Write-UjInformation -Message '[DryRun] Clear QoS markers.'
  } else {
    if ($PSCmdlet.ShouldProcess('Managed QoS policies', 'Remove QoS_* policies')) {
      Remove-UjManagedQosPolicy
    }
    if (Test-Path -Path $qosRegKey) {
      if ($PSCmdlet.ShouldProcess($qosRegKey, 'Remove Do not use NLA registry value')) {
        Remove-ItemProperty -Path $qosRegKey -Name 'Do not use NLA' -ErrorAction SilentlyContinue
      }
    }
  }

  $netshCommands = @(
    @('int', 'tcp', 'set', 'global', 'autotuninglevel=normal'),
    @('interface', 'teredo', 'set', 'state', 'default'),
    @('int', 'udp', 'set', 'global', 'uro=enabled'),
    @('int', 'tcp', 'set', 'supplemental', 'internet', 'icw=default'),
    @('int', 'tcp', 'set', 'supplemental', 'internet', 'minrto=default'),
    @('int', 'tcp', 'set', 'supplemental', 'internet', 'delayedacktimeout=default'),
    @('int', 'tcp', 'set', 'supplemental', 'internet', 'delayedackfrequency=default'),
    @('int', 'tcp', 'set', 'supplemental', 'internet', 'rack=disabled'),
    @('int', 'tcp', 'set', 'supplemental', 'internet', 'taillossprobe=disabled'),
    @('int', 'tcp', 'set', 'global', 'prr=enabled'),
    @('int', 'tcp', 'set', 'global', 'hystart=enabled')
  )

  foreach ($netshArgs in $netshCommands) {
    if ($DryRun) {
      Write-UjInformation -Message ("[DryRun] netsh {0}" -f ($netshArgs -join ' '))
      continue
    }

    if ($PSCmdlet.ShouldProcess('netsh', ($netshArgs -join ' '))) {
      try {
        $null = & netsh @netshArgs 2>&1
        if ($LASTEXITCODE -ne 0) {
          Write-Warning -Message ("Could not reset a Windows network setting (netsh {0}). This setting may not be available on your Windows version (error code {1})." -f ($netshArgs -join ' '), $LASTEXITCODE)
        }
      } catch {
        Write-Warning -Message ("A Windows network setting could not be reset: {0}" -f $_.Exception.Message)
      }
    }
  }

  try {
    $ts = Get-NetTCPSetting -SettingName Internet -ErrorAction SilentlyContinue
    if ($ts) {
      if ($DryRun) {
        Write-UjInformation -Message '[DryRun] Restore TCP Internet setting defaults (CNG/ECN)'
      } elseif ($PSCmdlet.ShouldProcess('NetTCPSetting Internet', 'Restore congestion & ECN defaults')) {
        Set-NetTCPSetting -SettingName Internet -CongestionProvider NewReno -EcnCapability Disabled | Out-Null
      }
    }
  } catch {
    Write-Verbose -Message 'TCP fallback settings apply failed.'
  }

  Write-UjInformation -Message 'Reset complete. Reboot recommended for full system synchronization.'
}
