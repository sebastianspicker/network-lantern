function Get-UjRestoreComponentResult {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]
    [ValidateSet('OK', 'Warn', 'Skipped')]
    [string]$Status,

    [Parameter()]
    [string]$Message = ''
  )

  return [pscustomobject]@{
    Status  = $Status
    Message = $Message
  }
}

function Resolve-UjRestoreStatus {
  [CmdletBinding()]
  [OutputType([string])]
  param(
    [Parameter()]
    $Result,

    [Parameter()]
    [string]$DefaultIfNull = 'Skipped'
  )

  if ($null -eq $Result) {
    return $DefaultIfNull
  }

  if ($Result -is [bool]) {
    if ($Result) {
      return 'OK'
    }
    return 'Warn'
  }

  if ($Result -is [System.Collections.IDictionary] -and $Result.Contains('Status')) {
    $statusValue = [string]$Result['Status']
    if ($statusValue -in @('OK', 'Warn', 'Skipped')) {
      return $statusValue
    }
  }

  if ($Result.PSObject -and ($Result.PSObject.Properties.Name -contains 'Status')) {
    $statusValue = [string]$Result.Status
    if ($statusValue -in @('OK', 'Warn', 'Skipped')) {
      return $statusValue
    }
  }

  return $DefaultIfNull
}

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

  $compReg = Export-UjRegistryKey -RegistryPath $script:UjRegistryPathSystemProfile -OutFile (Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileSystemProfile)
  $manifest.Components['SystemProfile'] = $compReg
  if (-not $compReg) { $backupHadFailure = $true }

  $compAfd = Export-UjRegistryKey -RegistryPath $script:UjRegistryPathAfdParameters -OutFile (Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileAfdParameters)
  $manifest.Components['AfdParameters'] = $compAfd
  if (-not $compAfd) { $backupHadFailure = $true }

  try {
    $policies = Get-UjManagedQosPolicy
    if ($policies) {
      $policies | Export-CliXml -Path (Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileQosOurs)
      $manifest.Components['QosPolicies'] = $true
    } else {
      Write-Verbose -Message 'No QoS policies found to backup.'
      $manifest.Components['QosPolicies'] = $true
    }
  } catch {
    Write-Warning -Message 'Could not back up your current QoS (network priority) settings. This is non-critical if you have not set up custom QoS rules before.'
    $manifest.Components['QosPolicies'] = $false
    $backupHadFailure = $true
  }

  try {
    $rows = foreach ($n in (Get-UjPhysicalUpAdapter)) {
      Get-NetAdapterAdvancedProperty -Name $n.Name |
        Select-Object @{ Name = 'Adapter'; Expression = { $n.Name } }, DisplayName, RegistryKeyword, DisplayValue, RegistryValue
    }
    if ($rows) {
      $rows | Export-Csv -NoTypeInformation -Path (Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileNicAdvanced)
      $manifest.Components['NicAdvanced'] = $true
    } else {
      $manifest.Components['NicAdvanced'] = $true
    }
  } catch {
    Write-Warning -Message 'Could not back up your network adapter settings. NIC tuning will still be applied but cannot be automatically undone via Restore.'
    $manifest.Components['NicAdvanced'] = $false
    $backupHadFailure = $true
  }

  try {
    Get-NetAdapterRsc | Select-Object Name, IPv4Enabled, IPv6Enabled |
      Export-Csv -NoTypeInformation -Path (Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileRsc)
    $manifest.Components['NicRsc'] = $true
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
        $guid | Out-File -FilePath (Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFilePowerplan) -Encoding utf8 -NoNewline
        $manifest.Components['PowerPlan'] = $true
      } else {
        $text | Out-File -FilePath (Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFilePowerplan) -Encoding utf8
        $manifest.Components['PowerPlan'] = $true
      }
    } else {
      $manifest.Components['PowerPlan'] = $true
    }
  } catch {
    Write-Verbose -Message ("Power plan snapshot failed: {0}" -f $_.Exception.Message)
    $manifest.Components['PowerPlan'] = $false
    $backupHadFailure = $true
  }

  $manifest | ConvertTo-Json | Out-File -FilePath (Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileManifest) -Encoding utf8

  if ($backupHadFailure) {
    return Get-UjRestoreComponentResult -Status 'Warn' -Message 'One or more backup components failed.'
  }

  return Get-UjRestoreComponentResult -Status 'OK' -Message 'Backup completed successfully.'
}

function Restore-UjRegistryFromBackup {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([pscustomobject])]
  param([Parameter(Mandatory)][string]$BackupFolder)

  $approvedCount = 0
  $deniedCount = 0
  $failedCount = 0

  $systemProfileReg = Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileSystemProfile
  if ($PSCmdlet.ShouldProcess($systemProfileReg, 'Import registry file')) {
    $approvedCount++
    if (-not (Import-UjRegistryFile -InFile $systemProfileReg)) { $failedCount++ }
  } else {
    $deniedCount++
  }

  $afdReg = Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileAfdParameters
  if ($PSCmdlet.ShouldProcess($afdReg, 'Import registry file')) {
    $approvedCount++
    if (-not (Import-UjRegistryFile -InFile $afdReg)) { $failedCount++ }
  } else {
    $deniedCount++
  }

  if ($approvedCount -eq 0) {
    return Get-UjRestoreComponentResult -Status 'Skipped' -Message 'Registry restore skipped by ShouldProcess.'
  }

  if ($failedCount -gt 0 -or $deniedCount -gt 0) {
    return Get-UjRestoreComponentResult -Status 'Warn' -Message 'One or more registry keys failed to restore or were denied by ShouldProcess.'
  }

  return Get-UjRestoreComponentResult -Status 'OK' -Message 'Registry keys restored.'
}

function Restore-UjQosFromBackup {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([pscustomobject])]
  param([Parameter(Mandatory)][string]$BackupFolder)

  $qosInventory = Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileQosOurs
  if (-not (Test-Path -Path $qosInventory)) {
    Write-Verbose -Message 'No QoS backup file found; skipping QoS restore.'
    return Get-UjRestoreComponentResult -Status 'Skipped' -Message 'QoS backup file not found.'
  }

  try {
    $qosItems = Import-CliXml -Path $qosInventory
  } catch {
    Write-Warning -Message ("Could not read the QoS backup file. It may be corrupted. File: {0} ({1})" -f $qosInventory, $_.Exception.Message)
    return Get-UjRestoreComponentResult -Status 'Warn' -Message 'QoS backup file could not be parsed.'
  }

  $hadFailure = $false
  $didWork = $false

  # Capture existing policies before deletion so we can attempt recovery on failure
  $existingPolicies = @()
  try {
    $existingPolicies = @(Get-UjManagedQosPolicy)
  } catch {
    Write-Verbose -Message 'Could not snapshot existing QoS policies before restore.'
  }

  if ($PSCmdlet.ShouldProcess('Managed QoS policies', 'Remove before restore')) {
    $didWork = $true
    Remove-UjManagedQosPolicy
  }

  foreach ($item in $qosItems) {
    $name = $item.Name
    $dscp = $script:UjDefaultDscp
    try {
      if ($item.PSObject.Properties.Name -contains 'DSCPAction' -and $null -ne $item.DSCPAction) {
        $dscpValue = [int]$item.DSCPAction
        if ($dscpValue -ge 0 -and $dscpValue -le 63) { $dscp = $dscpValue }
      } elseif ($item.PSObject.Properties.Name -contains 'DSCPValue' -and $null -ne $item.DSCPValue) {
        $dscpValue = [int]$item.DSCPValue
        if ($dscpValue -ge 0 -and $dscpValue -le 63) { $dscp = $dscpValue }
      }
    } catch {
      Write-Verbose -Message ("Failed to parse DSCP for policy {0}, using default" -f $name)
    }

    $proto = 'UDP'
    if ($item.PSObject.Properties.Name -contains 'IPProtocolMatchCondition' -and $item.IPProtocolMatchCondition) {
      $proto = [string]$item.IPProtocolMatchCondition
    }

    $portHandled = $false
    if ($item.PSObject.Properties.Name -contains 'IPPortMatchCondition') {
      try {
        $portValue = [int]$item.IPPortMatchCondition
        if ($portValue -gt 0 -and $portValue -le 65535) {
          if ($PSCmdlet.ShouldProcess($name, 'Recreate NetQosPolicy (port-based)')) {
            $didWork = $true
            try {
              New-NetQosPolicy -Name $name -IPPortMatchCondition ([uint16]$portValue) -IPProtocolMatchCondition $proto -DSCPAction ([sbyte]$dscp) -NetworkProfile All | Out-Null
            } catch {
              $hadFailure = $true
              Write-Warning -Message ("Could not restore network priority rule '{0}': {1}" -f $name, $_.Exception.Message)
            }
          }
          $portHandled = $true
        }
      } catch {
        Write-Verbose -Message ("Failed to parse port for policy {0}" -f $name)
      }
    }

    if (-not $portHandled -and $item.PSObject.Properties.Name -contains 'AppPathNameMatchCondition' -and $item.AppPathNameMatchCondition) {
      if ($PSCmdlet.ShouldProcess($name, 'Recreate NetQosPolicy (app-based)')) {
        $didWork = $true
        try {
          New-NetQosPolicy -Name $name -AppPathNameMatchCondition ([string]$item.AppPathNameMatchCondition) -DSCPAction ([sbyte]$dscp) -NetworkProfile All | Out-Null
        } catch {
          $hadFailure = $true
          Write-Warning -Message ("Could not restore network priority rule '{0}' for app: {1}" -f $name, $_.Exception.Message)
        }
      }
    }
  }

  if (-not $didWork) {
    return Get-UjRestoreComponentResult -Status 'Skipped' -Message 'QoS restore skipped by ShouldProcess.'
  }

  if ($hadFailure) {
    Write-Warning -Message 'Some network priority rules could not be restored. Attempting to recover your previous rules...'
    foreach ($orig in $existingPolicies) {
      try {
        $existing = Get-NetQosPolicy -Name $orig.Name -ErrorAction SilentlyContinue
        if (-not $existing) {
          if ($orig.PSObject.Properties.Name -contains 'IPPortMatchCondition' -and $orig.IPPortMatchCondition) {
            $origProto = if ($orig.PSObject.Properties.Name -contains 'IPProtocolMatchCondition' -and $orig.IPProtocolMatchCondition) { [string]$orig.IPProtocolMatchCondition } else { 'UDP' }
            $origDscp = if ($orig.PSObject.Properties.Name -contains 'DSCPAction' -and $null -ne $orig.DSCPAction) { [sbyte]$orig.DSCPAction } else { [sbyte]$script:UjDefaultDscp }
            New-NetQosPolicy -Name $orig.Name -IPPortMatchCondition ([uint16]$orig.IPPortMatchCondition) -IPProtocolMatchCondition $origProto -DSCPAction $origDscp -NetworkProfile All -ErrorAction Stop | Out-Null
          } elseif ($orig.PSObject.Properties.Name -contains 'AppPathNameMatchCondition' -and $orig.AppPathNameMatchCondition) {
            $origDscp = if ($orig.PSObject.Properties.Name -contains 'DSCPAction' -and $null -ne $orig.DSCPAction) { [sbyte]$orig.DSCPAction } else { [sbyte]$script:UjDefaultDscp }
            New-NetQosPolicy -Name $orig.Name -AppPathNameMatchCondition ([string]$orig.AppPathNameMatchCondition) -DSCPAction $origDscp -NetworkProfile All -ErrorAction Stop | Out-Null
          }
        }
      } catch {
        Write-Warning -Message ("Could not recover original network priority rule '{0}': {1}" -f $orig.Name, $_.Exception.Message)
      }
    }
    return Get-UjRestoreComponentResult -Status 'Warn' -Message 'One or more QoS policies failed to restore.'
  }

  return Get-UjRestoreComponentResult -Status 'OK' -Message 'QoS policies restored.'
}

function Restore-UjNicFromBackup {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([pscustomobject])]
  param([Parameter(Mandatory)][string]$BackupFolder)

  $csv = Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileNicAdvanced
  if (-not (Test-Path -Path $csv)) {
    return Get-UjRestoreComponentResult -Status 'Skipped' -Message 'NIC advanced backup file not found.'
  }

  $hadFailure = $false
  $didWork = $false
  try {
    $data = Import-Csv -Path $csv
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
  if (-not (Test-Path -Path $rscFile)) {
    return Get-UjRestoreComponentResult -Status 'Skipped' -Message 'RSC backup file not found.'
  }

  $didWork = $false
  try {
    foreach ($row in (Import-Csv -Path $rscFile)) {
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
  if (-not (Test-Path -Path $powerPlanFile)) {
    return Get-UjRestoreComponentResult -Status 'Skipped' -Message 'Power plan backup file not found.'
  }

  $text = Get-Content -Path $powerPlanFile -Raw
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

  if ($DryRun) {
    Write-UjInformation -Message '[DryRun] Skip restore (no writes).'
    return [ordered]@{
      Registry    = 'Skipped'
      Qos         = 'Skipped'
      NicAdvanced = 'Skipped'
      Rsc         = 'Skipped'
      PowerPlan   = 'Skipped'
    }
  }

  $manifestPath = Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileManifest
  if (Test-Path -Path $manifestPath) {
    try {
      $manifest = Get-Content -Path $manifestPath -Raw | ConvertFrom-Json
      Write-UjInformation -Message ("Validated backup manifest (Timestamp: {0})" -f $manifest.Timestamp)
    } catch {
      Write-Warning -Message 'Could not read the backup summary file. The backup may still work, but some details could not be verified.'
    }
  } else {
    Write-Warning -Message 'No backup summary file found. The backup may be incomplete or from an older version of this tool. Restore will proceed but some components may be skipped.'
  }

  $registryResult = Restore-UjRegistryFromBackup -BackupFolder $BackupFolder
  $qosResult = Restore-UjQosFromBackup -BackupFolder $BackupFolder
  $nicResult = Restore-UjNicFromBackup -BackupFolder $BackupFolder
  $rscResult = Restore-UjRscFromBackup -BackupFolder $BackupFolder
  $powerResult = Restore-UjPowerPlanFromBackup -BackupFolder $BackupFolder

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
}
