[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSShouldProcess', '', Justification = 'Restore-UjState delegates mutation decisions to component functions that implement ShouldProcess.')]
param()

function Restore-UjState {
  [CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
  [OutputType([System.Collections.Specialized.OrderedDictionary])]
  param(
    [Parameter(Mandatory)]
    [string]$BackupFolder,

    [Parameter()]
    [switch]$DryRun
  )

  Write-UjInformation -Message 'Restoring previous state ...'

  $manifestCheck = Read-UjBackupManifest -BackupFolder $BackupFolder
  if ($manifestCheck.Status -eq 'OK') {
    Write-UjInformation -Message ("Validated backup manifest (Timestamp: {0})" -f $manifestCheck.Manifest.Timestamp)
  } else {
    Write-Warning -Message ("Restore blocked: {0}" -f $manifestCheck.Message)
    return [ordered]@{
      Manifest    = 'Warn'
      Registry    = 'Skipped'
      Qos         = 'Skipped'
      NicAdvanced = 'Skipped'
      Rsc         = 'Skipped'
      PowerPlan   = 'Skipped'
    }
  }

  if ($DryRun) {
    Write-UjInformation -Message '[DryRun] Backup manifest verified; skip restore writes.'
    return [ordered]@{
      Manifest    = 'OK'
      Registry    = 'Skipped'
      Qos         = 'Skipped'
      NicAdvanced = 'Skipped'
      Rsc         = 'Skipped'
      PowerPlan   = 'Skipped'
    }
  }

  try {
    $stagingSession = Copy-UjVerifiedBackupToStaging -BackupFolder $BackupFolder -Manifest $manifestCheck.Manifest
  } catch {
    Write-Warning -Message ("Restore blocked while staging verified artifacts: {0}" -f $_.Exception.Message)
    return [ordered]@{
      Manifest    = 'Warn'
      Registry    = 'Skipped'
      Qos         = 'Skipped'
      NicAdvanced = 'Skipped'
      Rsc         = 'Skipped'
      PowerPlan   = 'Skipped'
    }
  }

  try {
    $restoreFolder = [string]$stagingSession.Path
    $restoreResults = [ordered]@{}
    $restoreOperations = @(
      @{
        Name = 'Registry'
        Invoke = {
          Restore-UjRegistryFromBackup `
            -BackupFolder $restoreFolder `
            -StagingSession $stagingSession `
            -Manifest $manifestCheck.Manifest
        }
      },
      @{ Name = 'Qos'; Invoke = { Restore-UjQosFromBackup -BackupFolder $restoreFolder } },
      @{ Name = 'NicAdvanced'; Invoke = { Restore-UjNicFromBackup -BackupFolder $restoreFolder } },
      @{ Name = 'Rsc'; Invoke = { Restore-UjRscFromBackup -BackupFolder $restoreFolder } },
      @{ Name = 'PowerPlan'; Invoke = { Restore-UjPowerPlanFromBackup -BackupFolder $restoreFolder } }
    )

    foreach ($operation in $restoreOperations) {
      try {
        Assert-UjRestoreStagingConsumerInvariant -Session $stagingSession -Manifest $manifestCheck.Manifest
      } catch {
        Write-Warning -Message ("Restore blocked before consumer '{0}' because the verified staging session changed: {1}" -f $operation.Name, $_.Exception.Message)
        return Get-UjStagingBlockedRestoreStatus -RestoreResults $restoreResults
      }

      try {
        $restoreResults[$operation.Name] = & $operation.Invoke
      } catch {
        if ($_.Exception.Data.Contains('NetworkLantern.RestoreStagingInvariant') -and
            [bool]$_.Exception.Data['NetworkLantern.RestoreStagingInvariant']) {
          Write-Warning -Message ("Restore blocked inside consumer '{0}' because the verified staging session changed: {1}" -f $operation.Name, $_.Exception.Message)
          $restoreResults[$operation.Name] = Get-UjRestoreComponentResult -Status 'Warn' -Message 'Restore consumer stopped after staging verification failed.'
          return Get-UjStagingBlockedRestoreStatus -RestoreResults $restoreResults
        }
        throw
      }
    }

    $registryResult = $restoreResults['Registry']
    $qosResult = $restoreResults['Qos']
    $nicResult = $restoreResults['NicAdvanced']
    $rscResult = $restoreResults['Rsc']
    $powerResult = $restoreResults['PowerPlan']

    $componentStatus = [ordered]@{
      Registry    = Resolve-UjRestoreStatus -Result $registryResult
      Qos         = Resolve-UjRestoreStatus -Result $qosResult
      NicAdvanced = Resolve-UjRestoreStatus -Result $nicResult
      Rsc         = Resolve-UjRestoreStatus -Result $rscResult
      PowerPlan   = Resolve-UjRestoreStatus -Result $powerResult
    }

    Write-UjInformation -Message (
      "Restore complete. Components: Registry={0}; QoS={1}; NicAdvanced={2}; RSC={3}; PowerPlan={4}. A reboot may be required for registry-based settings." -f
      $componentStatus.Registry,
      $componentStatus.Qos,
      $componentStatus.NicAdvanced,
      $componentStatus.Rsc,
      $componentStatus.PowerPlan
    )

    foreach ($entry in @(
        @{ Name = 'Registry'; Result = $registryResult },
        @{ Name = 'QoS'; Result = $qosResult },
        @{ Name = 'NIC'; Result = $nicResult },
        @{ Name = 'RSC'; Result = $rscResult },
        @{ Name = 'PowerPlan'; Result = $powerResult }
      )) {
      $entryResult = $entry['Result']
      if ($null -ne $entryResult -and
          $entryResult.PSObject -and
          ($entryResult.PSObject.Properties.Match('Message').Count -gt 0) -and
          -not [string]::IsNullOrWhiteSpace([string]$entryResult.Message)) {
        Write-Verbose -Message ("Restore detail [{0}]: {1}" -f $entry['Name'], $entryResult.Message)
      }
    }

    return $componentStatus
  } finally {
    Close-UjRestoreStagingSession -Session $stagingSession
  }
}
