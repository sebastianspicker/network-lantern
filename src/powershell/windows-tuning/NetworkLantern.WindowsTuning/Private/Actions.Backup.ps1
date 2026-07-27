function Backup-UjState {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]
    [string]$BackupFolder,

    [Parameter()]
    [switch]$DryRun
  )

  Write-UjInformation -Message 'Backing up current state ...'
  if ($DryRun) {
    Write-UjInformation -Message '[DryRun] Skip backup (no writes).'
    return Get-UjRestoreComponentResult -Status 'Skipped' -Message 'Backup skipped (DryRun).'
  }

  if (-not $PSCmdlet.ShouldProcess($BackupFolder, 'Write backup artifacts')) {
    return Get-UjRestoreComponentResult -Status 'Skipped' -Message 'Backup skipped by ShouldProcess.'
  }

  New-UjDirectory -Path $BackupFolder | Out-Null
  $backupHadFailure = $false
  $manifest = @{
    Timestamp  = (Get-Date -Format 'o')
    Components = @{}
  }
  $metadata = Get-UjBackupManifestMetadata
  foreach ($key in $metadata.Keys) {
    $manifest[$key] = $metadata[$key]
  }

  $compReg = Export-UjRegistryKey -RegistryPath $script:UjRegistryPathSystemProfile -OutFile (Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileSystemProfile)
  $manifest.Components['SystemProfile'] = $compReg
  if (-not $compReg) { $backupHadFailure = $true }

  $compAfd = Export-UjRegistryKey -RegistryPath $script:UjRegistryPathAfdParameters -OutFile (Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileAfdParameters)
  $manifest.Components['AfdParameters'] = $compAfd
  if (-not $compAfd) { $backupHadFailure = $true }

  try {
    $policies = @(Get-UjManagedQosPolicy -ErrorOnFailure)
    $qosBackupPath = Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileQosOurs
    if ($policies.Count -eq 0) {
      Write-Verbose -Message 'No QoS policies found to backup.'
      Export-CliXml -LiteralPath $qosBackupPath -InputObject ([System.Collections.ArrayList]::new())
    } else {
      Export-CliXml -LiteralPath $qosBackupPath -InputObject $policies
    }
    $manifest.Components['QosPolicies'] = $true
  } catch {
    Write-Warning -Message 'Could not back up your current QoS (network priority) settings. This is non-critical if you have not set up custom QoS rules before.'
    $manifest.Components['QosPolicies'] = $false
    $backupHadFailure = $true
  }

  try {
    $rows = @(
      foreach ($n in (Get-UjPhysicalUpAdapter)) {
        Get-NetAdapterAdvancedProperty -Name $n.Name |
          Select-Object @{ Name = 'Adapter'; Expression = { $n.Name } }, DisplayName, RegistryKeyword, DisplayValue, RegistryValue
      }
    )
    if ($rows.Count -gt 0) {
      $rows | Export-Csv -NoTypeInformation -LiteralPath (Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileNicAdvanced)
      $manifest.Components['NicAdvanced'] = $true
    } else {
      $manifest.Components['NicAdvanced'] = $false
      $backupHadFailure = $true
    }
  } catch {
    Write-Warning -Message 'Could not back up your network adapter settings. NIC tuning will still be applied but cannot be automatically undone via Restore.'
    $manifest.Components['NicAdvanced'] = $false
    $backupHadFailure = $true
  }

  try {
    $rscRows = @(Get-NetAdapterRsc | Select-Object Name, IPv4Enabled, IPv6Enabled)
    if ($rscRows.Count -gt 0) {
      $rscRows | Export-Csv -NoTypeInformation -LiteralPath (Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileRsc)
      $manifest.Components['NicRsc'] = $true
    } else {
      $manifest.Components['NicRsc'] = $false
      $backupHadFailure = $true
    }
  } catch {
    Write-Verbose -Message 'RSC snapshot failed.'
    $manifest.Components['NicRsc'] = $false
    $backupHadFailure = $true
  }

  try {
    $powerPlanOutput = & powercfg /GetActiveScheme 2>&1
    if ($LASTEXITCODE -eq 0 -and $powerPlanOutput) {
      $text = $powerPlanOutput -join "`n"
      $guid = Get-UjGuidFromText -Text $text

      if ($guid) {
        Set-Content -LiteralPath (Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFilePowerplan) -Value $guid -Encoding utf8 -NoNewline
        $manifest.Components['PowerPlan'] = $true
      } else {
        $manifest.Components['PowerPlan'] = $false
        $backupHadFailure = $true
      }
    } else {
      $manifest.Components['PowerPlan'] = $false
      $backupHadFailure = $true
    }
  } catch {
    Write-Verbose -Message ("Power plan snapshot failed: {0}" -f $_.Exception.Message)
    $manifest.Components['PowerPlan'] = $false
    $backupHadFailure = $true
  }

  $manifest.ArtifactDigests = Get-UjBackupArtifactDigests -BackupFolder $BackupFolder -Manifest $manifest
  $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileManifest) -Encoding utf8

  $backupVerification = Read-UjBackupManifest -BackupFolder $BackupFolder
  if ($backupVerification.Status -ne 'OK') {
    $backupHadFailure = $true
    Write-Warning -Message ("Backup verification failed: {0}" -f $backupVerification.Message)
  }

  if ($backupHadFailure) {
    return Get-UjRestoreComponentResult -Status 'Warn' -Message 'One or more backup components failed.'
  }

  return Get-UjRestoreComponentResult -Status 'OK' -Message 'Backup completed successfully.'
}

