function Restore-UjRegistryFromBackup {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)][string]$BackupFolder,
    [Parameter(Mandatory)]$StagingSession,
    [Parameter(Mandatory)][hashtable]$Manifest
  )

  $approvedCount = 0
  $deniedCount = 0
  $failedCount = 0

  $registryArtifacts = @(
      @{ Path = (Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileSystemProfile); Contract = 'SystemProfile' },
      @{ Path = (Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileAfdParameters); Contract = 'AfdParameters' }
    ) | Where-Object { Test-Path -LiteralPath $_.Path -PathType Leaf }

  foreach ($artifact in $registryArtifacts) {
    Assert-UjRestoreStagingConsumerInvariant -Session $StagingSession -Manifest $Manifest
    $validation = Test-UjRegistryBackupFile -InFile $artifact.Path -ContractName $artifact.Contract
    if (-not $validation.IsApproved) {
      Write-Warning -Message ("Refusing to restore an unapproved registry backup: {0}" -f $validation.Message)
      return Get-UjRestoreComponentResult -Status 'Warn' -Message $validation.Message
    }
  }

  foreach ($artifact in $registryArtifacts) {
    if ($PSCmdlet.ShouldProcess($artifact.Path, 'Import registry file')) {
      Assert-UjRestoreStagingConsumerInvariant -Session $StagingSession -Manifest $Manifest
      $approvedCount++
      if (-not (Import-UjRegistryFile -InFile $artifact.Path -ContractName $artifact.Contract)) { $failedCount++ }
    } else {
      $deniedCount++
    }
  }

  if ($approvedCount -eq 0) {
    return Get-UjRestoreComponentResult -Status 'Skipped' -Message 'Registry restore skipped by ShouldProcess.'
  }

  if ($failedCount -gt 0 -or $deniedCount -gt 0) {
    return Get-UjRestoreComponentResult -Status 'Warn' -Message 'One or more registry keys failed to restore or were denied by ShouldProcess.'
  }

  return Get-UjRestoreComponentResult -Status 'OK' -Message 'Registry keys restored.'
}

function Restore-UjNicFromBackup {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([pscustomobject])]
  param([Parameter(Mandatory)][string]$BackupFolder)

  $csv = Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileNicAdvanced
  if (-not (Test-Path -LiteralPath $csv -PathType Leaf)) {
    return Get-UjRestoreComponentResult -Status 'Skipped' -Message 'NIC advanced backup file not found.'
  }

  $hadFailure = $false
  $didWork = $false
  try {
    $data = Import-Csv -LiteralPath $csv
    $firstRow = $data | Select-Object -First 1
    if (-not $firstRow -or -not ($firstRow.PSObject.Properties.Name -contains 'Adapter')) {
      Write-Warning -Message 'Could not restore network adapter settings: the backup file is missing or corrupted. You can use "Reset to Defaults" instead to return to stock settings.'
      return Get-UjRestoreComponentResult -Status 'Warn' -Message 'NIC backup CSV is invalid.'
    }

    $adapters = $data | Select-Object -ExpandProperty Adapter -Unique
    foreach ($adapter in $adapters) {
      foreach ($property in ($data | Where-Object { $_.Adapter -eq $adapter })) {
        try {
          if (-not [string]::IsNullOrEmpty($property.RegistryKeyword) -and -not [string]::IsNullOrEmpty($property.RegistryValue)) {
            if ($PSCmdlet.ShouldProcess($adapter, ("Restore NIC advanced property keyword: {0}" -f $property.RegistryKeyword))) {
              $didWork = $true
              Set-NetAdapterAdvancedProperty -Name $adapter -RegistryKeyword $property.RegistryKeyword -RegistryValue $property.RegistryValue -NoRestart -ErrorAction Stop | Out-Null
            }
            continue
          }
          if (-not [string]::IsNullOrEmpty($property.DisplayName)) {
            if ($PSCmdlet.ShouldProcess($adapter, ("Restore NIC advanced property: {0}" -f $property.DisplayName))) {
              $didWork = $true
              Set-NetAdapterAdvancedProperty -Name $adapter -DisplayName $property.DisplayName -DisplayValue $property.DisplayValue -NoRestart -ErrorAction Stop | Out-Null
            }
            continue
          }
        } catch {
          $hadFailure = $true
          Write-Verbose -Message ("NIC property restore failed: {0} ({1})" -f $adapter, $property.DisplayName)
        }
      }
    }
  } catch {
    Write-Warning -Message 'Something went wrong restoring network adapter settings. You can use "Reset to Defaults" to return to stock settings.'
    return Get-UjRestoreComponentResult -Status 'Warn' -Message 'NIC advanced restore failed during CSV import or parsing.'
  }

  if (-not $didWork) {
    return Get-UjRestoreComponentResult -Status 'Skipped' -Message 'NIC advanced restore skipped by ShouldProcess.'
  }

  if ($hadFailure) {
    return Get-UjRestoreComponentResult -Status 'Warn' -Message 'One or more NIC properties failed to restore.'
  }

  return Get-UjRestoreComponentResult -Status 'OK' -Message 'NIC advanced properties restored.'
}

function Restore-UjRscFromBackup {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([pscustomobject])]
  param([Parameter(Mandatory)][string]$BackupFolder)

  $rscFile = Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileRsc
  if (-not (Test-Path -LiteralPath $rscFile -PathType Leaf)) {
    return Get-UjRestoreComponentResult -Status 'Skipped' -Message 'RSC backup file not found.'
  }

  $didWork = $false
  try {
    foreach ($row in (Import-Csv -LiteralPath $rscFile)) {
      if ([string]::IsNullOrWhiteSpace($row.Name)) {
        continue
      }
      $ipv4Enabled = [string]$row.IPv4Enabled -ieq 'True'
      $ipv6Enabled = [string]$row.IPv6Enabled -ieq 'True'

      if ($PSCmdlet.ShouldProcess($row.Name, 'Restore NetAdapterRsc IPv4/IPv6 state')) {
        $didWork = $true
        if ($ipv4Enabled) {
          Enable-NetAdapterRsc -Name $row.Name -IPv4 -ErrorAction SilentlyContinue | Out-Null
        } else {
          Disable-NetAdapterRsc -Name $row.Name -IPv4 -ErrorAction SilentlyContinue | Out-Null
        }
        if ($ipv6Enabled) {
          Enable-NetAdapterRsc -Name $row.Name -IPv6 -ErrorAction SilentlyContinue | Out-Null
        } else {
          Disable-NetAdapterRsc -Name $row.Name -IPv6 -ErrorAction SilentlyContinue | Out-Null
        }
      }
    }
  } catch {
    Write-Verbose -Message 'RSC restore failed.'
    return Get-UjRestoreComponentResult -Status 'Warn' -Message 'RSC restore failed.'
  }

  if (-not $didWork) {
    return Get-UjRestoreComponentResult -Status 'Skipped' -Message 'RSC restore skipped by ShouldProcess.'
  }

  return Get-UjRestoreComponentResult -Status 'OK' -Message 'RSC state restored.'
}

function Restore-UjPowerPlanFromBackup {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([pscustomobject])]
  param([Parameter(Mandatory)][string]$BackupFolder)

  $powerPlanFile = Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFilePowerplan
  if (-not (Test-Path -LiteralPath $powerPlanFile -PathType Leaf)) {
    return Get-UjRestoreComponentResult -Status 'Skipped' -Message 'Power plan backup file not found.'
  }

  $text = Get-Content -LiteralPath $powerPlanFile -Raw
  $guid = Get-UjGuidFromText -Text $text

  if (-not $guid) {
    Write-Warning -Message 'Could not restore your previous power plan: the backup file does not contain a valid plan ID. Your power plan was not changed.'
    return Get-UjRestoreComponentResult -Status 'Warn' -Message 'Power plan GUID missing or invalid.'
  }

  if (-not $PSCmdlet.ShouldProcess($guid, 'Restore power plan')) {
    return Get-UjRestoreComponentResult -Status 'Skipped' -Message 'Power plan restore skipped by ShouldProcess.'
  }

  try {
    $null = & powercfg /S $guid 2>&1
    if ($LASTEXITCODE -ne 0) {
      Write-Warning -Message ("Could not restore your previous power plan. The saved plan may have been removed from this PC (error code {0})." -f $LASTEXITCODE)
      return Get-UjRestoreComponentResult -Status 'Warn' -Message ('powercfg /S exited with non-zero status.')
    }
  } catch {
    Write-Verbose -Message ("Power plan restore failed: {0}" -f $_.Exception.Message)
    return Get-UjRestoreComponentResult -Status 'Warn' -Message 'Power plan restore threw an exception.'
  }

  return Get-UjRestoreComponentResult -Status 'OK' -Message 'Power plan restored.'
}

