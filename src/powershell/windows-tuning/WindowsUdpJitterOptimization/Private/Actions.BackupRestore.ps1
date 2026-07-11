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

function Get-UjBackupManifestMetadata {
  [CmdletBinding()]
  [OutputType([hashtable])]
  param()

  $moduleVersion = $null
  try {
    $moduleVersion = (Get-Module -Name WindowsUdpJitterOptimization -ErrorAction SilentlyContinue | Select-Object -First 1).Version.ToString()
  } catch {
    $moduleVersion = $null
  }

  return @{
    SchemaVersion = $script:UjBackupSchemaVersion
    ToolName      = 'network-diagnostics-suite'
    MachineName   = [System.Environment]::MachineName
    Platform      = [System.Environment]::OSVersion.Platform.ToString()
    OsVersion     = [System.Environment]::OSVersion.VersionString
    ModuleVersion = $moduleVersion
  }
}

function Get-UjBackupArtifactFileMap {
  [CmdletBinding()]
  [OutputType([hashtable])]
  param()

  return @{
    SystemProfile = @($script:UjBackupFileSystemProfile)
    AfdParameters = @($script:UjBackupFileAfdParameters)
    QosPolicies  = @($script:UjBackupFileQosOurs)
    NicAdvanced  = @($script:UjBackupFileNicAdvanced)
    NicRsc        = @($script:UjBackupFileRsc)
    PowerPlan     = @($script:UjBackupFilePowerplan)
  }
}

function Get-UjKnownBackupArtifactNames {
  [CmdletBinding()]
  [OutputType([string[]])]
  param()

  $names = [System.Collections.Generic.List[string]]::new()
  foreach ($entry in (Get-UjBackupArtifactFileMap).GetEnumerator()) {
    foreach ($name in @($entry.Value)) {
      if (-not [string]::IsNullOrWhiteSpace($name) -and -not $names.Contains($name)) {
        $names.Add($name) | Out-Null
      }
    }
  }
  return $names.ToArray()
}

function Test-UjSafeBackupArtifactName {
  [CmdletBinding()]
  [OutputType([bool])]
  param(
    [Parameter(Mandatory)][string]$Name
  )

  if ([string]::IsNullOrWhiteSpace($Name)) { return $false }
  if ($Name -match '[\\/:\x00-\x1f]') { return $false }
  if ($Name -eq '.' -or $Name -eq '..') { return $false }
  return $true
}

function Get-UjExpectedBackupArtifactNames {
  [CmdletBinding()]
  [OutputType([string[]])]
  param(
    [Parameter(Mandatory)][hashtable]$Manifest
  )

  $expected = [System.Collections.Generic.List[string]]::new()
  $fileMap = Get-UjBackupArtifactFileMap
  $components = if ($Manifest.ContainsKey('Components') -and $Manifest['Components'] -is [System.Collections.IDictionary]) {
    $Manifest['Components']
  } else {
    @{}
  }

  foreach ($componentName in $fileMap.Keys) {
    $componentEnabled = $false
    if ($components.Contains($componentName)) {
      $componentEnabled = [bool]$components[$componentName]
    }
    if (-not $componentEnabled) { continue }

    foreach ($fileName in @($fileMap[$componentName])) {
      if (-not $expected.Contains($fileName)) {
        $expected.Add($fileName) | Out-Null
      }
    }
  }

  return $expected.ToArray()
}

function Get-UjBackupArtifactDigests {
  [CmdletBinding()]
  [OutputType([hashtable])]
  param(
    [Parameter(Mandatory)][string]$BackupFolder,
    [Parameter(Mandatory)][hashtable]$Manifest
  )

  $digests = @{}
  foreach ($fileName in (Get-UjExpectedBackupArtifactNames -Manifest $Manifest)) {
    $artifactPath = Join-Path -Path $BackupFolder -ChildPath $fileName
    if (Test-Path -LiteralPath $artifactPath -PathType Leaf) {
      $digests[$fileName] = (Get-FileHash -LiteralPath $artifactPath -Algorithm SHA256).Hash
    }
  }
  return $digests
}

function Test-UjBackupPathTrust {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)][string]$BackupFolder,
    [Parameter(Mandatory)][hashtable]$Manifest
  )

  $isWindowsRuntime = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)
  if (-not $isWindowsRuntime) {
    return [pscustomobject]@{ IsTrusted = $true; Message = '' }
  }

  $currentUserSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
  $trustedOwnerSids = @(
    $currentUserSid,
    'S-1-5-18',
    'S-1-5-32-544'
  )
  $broadWriterSids = @(
    'S-1-1-0',
    'S-1-5-11',
    'S-1-5-32-545',
    'S-1-5-32-546'
  )
  $writeRights = [System.Security.AccessControl.FileSystemRights]::Write -bor
    [System.Security.AccessControl.FileSystemRights]::WriteData -bor
    [System.Security.AccessControl.FileSystemRights]::CreateFiles -bor
    [System.Security.AccessControl.FileSystemRights]::AppendData -bor
    [System.Security.AccessControl.FileSystemRights]::Modify -bor
    [System.Security.AccessControl.FileSystemRights]::FullControl -bor
    [System.Security.AccessControl.FileSystemRights]::ChangePermissions -bor
    [System.Security.AccessControl.FileSystemRights]::TakeOwnership

  $pathsToCheck = [System.Collections.Generic.List[string]]::new()
  $pathsToCheck.Add($BackupFolder) | Out-Null
  $manifestPath = Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileManifest
  if (Test-Path -LiteralPath $manifestPath -PathType Leaf) {
    $pathsToCheck.Add($manifestPath) | Out-Null
  }
  foreach ($fileName in (Get-UjExpectedBackupArtifactNames -Manifest $Manifest)) {
    $artifactPath = Join-Path -Path $BackupFolder -ChildPath $fileName
    if (Test-Path -LiteralPath $artifactPath -PathType Leaf) {
      $pathsToCheck.Add($artifactPath) | Out-Null
    }
  }

  foreach ($path in $pathsToCheck) {
    try {
      $acl = Get-Acl -LiteralPath $path
      $ownerSid = ([System.Security.Principal.NTAccount]$acl.Owner).Translate([System.Security.Principal.SecurityIdentifier]).Value
      if ($ownerSid -notin $trustedOwnerSids) {
        return [pscustomobject]@{ IsTrusted = $false; Message = "Backup path owner is not trusted: $path" }
      }

      foreach ($rule in $acl.Access) {
        if ($rule.AccessControlType -ne [System.Security.AccessControl.AccessControlType]::Allow) {
          continue
        }

        $identitySid = $null
        try {
          $identitySid = $rule.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value
        } catch {
          Write-Verbose -Message ("Could not translate ACL identity '{0}' on '{1}'." -f $rule.IdentityReference, $path)
          continue
        }

        if ($identitySid -in $broadWriterSids -and (($rule.FileSystemRights -band $writeRights) -ne 0)) {
          return [pscustomobject]@{ IsTrusted = $false; Message = "Backup path is writable by a broad local group: $path" }
        }
      }
    } catch {
      return [pscustomobject]@{ IsTrusted = $false; Message = "Could not validate backup path trust: $path" }
    }
  }

  return [pscustomobject]@{ IsTrusted = $true; Message = '' }
}

function Test-UjBackupArtifactDigests {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)][string]$BackupFolder,
    [Parameter(Mandatory)][hashtable]$Manifest
  )

  if (-not $Manifest.ContainsKey('ArtifactDigests') -or -not ($Manifest['ArtifactDigests'] -is [System.Collections.IDictionary])) {
    return [pscustomobject]@{ IsValid = $false; Message = 'Backup manifest is missing artifact digests.' }
  }

  $expectedNames = @(Get-UjExpectedBackupArtifactNames -Manifest $Manifest)
  $knownNames = @(Get-UjKnownBackupArtifactNames)
  $digests = $Manifest['ArtifactDigests']

  foreach ($name in $digests.Keys) {
    $artifactName = [string]$name
    if (-not (Test-UjSafeBackupArtifactName -Name $artifactName)) {
      return [pscustomobject]@{ IsValid = $false; Message = "Backup manifest contains an unsafe artifact name: $artifactName" }
    }
    if ($artifactName -notin $knownNames) {
      return [pscustomobject]@{ IsValid = $false; Message = "Backup manifest contains an unknown artifact: $artifactName" }
    }
    if ($artifactName -notin $expectedNames) {
      return [pscustomobject]@{ IsValid = $false; Message = "Backup manifest contains an artifact for a disabled component: $artifactName" }
    }

    $expectedHash = [string]$digests[$name]
    if ($expectedHash -cnotmatch '^[0-9A-F]{64}$') {
      return [pscustomobject]@{ IsValid = $false; Message = "Backup manifest contains an invalid SHA-256 digest for: $artifactName" }
    }

    $artifactPath = Join-Path -Path $BackupFolder -ChildPath $artifactName
    if (-not (Test-Path -LiteralPath $artifactPath -PathType Leaf)) {
      return [pscustomobject]@{ IsValid = $false; Message = "Backup artifact listed in manifest is missing: $artifactName" }
    }

    $actualHash = (Get-FileHash -LiteralPath $artifactPath -Algorithm SHA256).Hash
    if ($actualHash -cne $expectedHash) {
      return [pscustomobject]@{ IsValid = $false; Message = "Backup artifact digest mismatch: $artifactName" }
    }
  }

  foreach ($artifactName in $knownNames) {
    $artifactPath = Join-Path -Path $BackupFolder -ChildPath $artifactName
    if (-not (Test-Path -LiteralPath $artifactPath -PathType Leaf)) { continue }
    if ($artifactName -notin $expectedNames) {
      return [pscustomobject]@{ IsValid = $false; Message = "Unexpected backup artifact for disabled component: $artifactName" }
    }
    if (-not $digests.Contains($artifactName)) {
      return [pscustomobject]@{ IsValid = $false; Message = "Backup artifact is not listed in manifest digests: $artifactName" }
    }
  }

  return [pscustomobject]@{ IsValid = $true; Message = '' }
}

function Read-UjBackupManifest {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)][string]$BackupFolder
  )

  $manifestPath = Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileManifest
  if (-not (Test-Path -Path $manifestPath)) {
    return [pscustomobject]@{ Status = 'Missing'; Message = 'No backup summary file found.'; Manifest = $null }
  }

  try {
    $manifest = Get-Content -Path $manifestPath -Raw | ConvertFrom-Json -AsHashtable
  } catch {
    return [pscustomobject]@{ Status = 'Invalid'; Message = 'Could not read the backup summary file.'; Manifest = $null }
  }

  if (-not $manifest.ContainsKey('SchemaVersion')) {
    return [pscustomobject]@{ Status = 'Invalid'; Message = 'Backup manifest is missing SchemaVersion.'; Manifest = $manifest }
  }
  if ([int]$manifest['SchemaVersion'] -gt [int]$script:UjBackupSchemaVersion) {
    return [pscustomobject]@{ Status = 'Incompatible'; Message = 'Backup manifest schema is newer than this module supports.'; Manifest = $manifest }
  }
  if ($manifest.ContainsKey('ToolName') -and [string]$manifest['ToolName'] -ne 'network-diagnostics-suite') {
    return [pscustomobject]@{ Status = 'Incompatible'; Message = 'Backup manifest tool name does not match this module.'; Manifest = $manifest }
  }

  $pathTrust = Test-UjBackupPathTrust -BackupFolder $BackupFolder -Manifest $manifest
  if (-not $pathTrust.IsTrusted) {
    return [pscustomobject]@{ Status = 'Invalid'; Message = $pathTrust.Message; Manifest = $manifest }
  }

  $artifactCheck = Test-UjBackupArtifactDigests -BackupFolder $BackupFolder -Manifest $manifest
  if (-not $artifactCheck.IsValid) {
    return [pscustomobject]@{ Status = 'Invalid'; Message = $artifactCheck.Message; Manifest = $manifest }
  }

  return [pscustomobject]@{ Status = 'OK'; Message = ''; Manifest = $manifest }
}

function ConvertTo-UjQosBackupSpec {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]$Item
  )

  if ([string]::IsNullOrWhiteSpace([string]$Item.Name)) {
    throw 'QoS backup item is missing Name.'
  }

  $dscp = [sbyte]$script:UjDefaultDscp
  if ($Item.PSObject.Properties.Name -contains 'DSCPAction' -and $null -ne $Item.DSCPAction) {
    $dscp = [sbyte][int]$Item.DSCPAction
  } elseif ($Item.PSObject.Properties.Name -contains 'DSCPValue' -and $null -ne $Item.DSCPValue) {
    $dscp = [sbyte][int]$Item.DSCPValue
  }

  if ($Item.PSObject.Properties.Name -contains 'IPPortMatchCondition' -and $Item.IPPortMatchCondition) {
    return [pscustomobject]@{
      Name = [string]$Item.Name
      Type = 'Port'
      Protocol = if ($Item.PSObject.Properties.Name -contains 'IPProtocolMatchCondition' -and $Item.IPProtocolMatchCondition) { [string]$Item.IPProtocolMatchCondition } else { 'UDP' }
      Port = [uint16][int]$Item.IPPortMatchCondition
      Dscp = $dscp
    }
  }

  if ($Item.PSObject.Properties.Name -contains 'AppPathNameMatchCondition' -and $Item.AppPathNameMatchCondition) {
    return [pscustomobject]@{
      Name = [string]$Item.Name
      Type = 'App'
      AppPath = [string]$Item.AppPathNameMatchCondition
      Dscp = $dscp
    }
  }

  throw "QoS backup item '$($Item.Name)' has no supported match condition."
}

function Test-UjQosSpecMatchesPolicy {
  [CmdletBinding()]
  [OutputType([bool])]
  param(
    [Parameter(Mandatory)][object]$Spec,
    [Parameter(Mandatory)][object]$Policy
  )

  $policyDscp = if ($Policy.PSObject.Properties.Name -contains 'DSCPAction' -and $null -ne $Policy.DSCPAction) { [int]$Policy.DSCPAction } else { [int]$script:UjDefaultDscp }
  if ($Spec.Type -eq 'Port') {
    return ([string]$Policy.Name -eq $Spec.Name -and [int]$Policy.IPPortMatchCondition -eq [int]$Spec.Port -and [string]$Policy.IPProtocolMatchCondition -eq $Spec.Protocol -and $policyDscp -eq [int]$Spec.Dscp)
  }

  return ([string]$Policy.Name -eq $Spec.Name -and [string]$Policy.AppPathNameMatchCondition -eq $Spec.AppPath -and $policyDscp -eq [int]$Spec.Dscp)
}

function New-UjQosPolicyFromSpec {
  [CmdletBinding()]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)][object]$Spec
  )

  if ($Spec.Type -eq 'Port') {
    New-NetQosPolicy -Name $Spec.Name -IPPortMatchCondition $Spec.Port -IPProtocolMatchCondition $Spec.Protocol -DSCPAction $Spec.Dscp -NetworkProfile All -ErrorAction Stop | Out-Null
    return
  }

  New-NetQosPolicy -Name $Spec.Name -AppPathNameMatchCondition $Spec.AppPath -DSCPAction $Spec.Dscp -NetworkProfile All -ErrorAction Stop | Out-Null
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
    $policies = Get-UjManagedQosPolicy -ErrorOnFailure
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

  $manifest.ArtifactDigests = Get-UjBackupArtifactDigests -BackupFolder $BackupFolder -Manifest $manifest
  $manifest | ConvertTo-Json -Depth 5 | Out-File -FilePath (Join-Path -Path $BackupFolder -ChildPath $script:UjBackupFileManifest) -Encoding utf8

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
  $desiredSpecs = @()
  try {
    $desiredSpecs = @($qosItems | ForEach-Object { ConvertTo-UjQosBackupSpec -Item $_ })
  } catch {
    Write-Warning -Message ("QoS backup validation failed before any policies were removed. {0}" -f $_.Exception.Message)
    return Get-UjRestoreComponentResult -Status 'Warn' -Message 'QoS backup validation failed.'
  }

  $existingPolicies = @()
  try {
    $existingPolicies = @(Get-UjManagedQosPolicy)
  } catch {
    Write-Verbose -Message 'Could not snapshot existing QoS policies before restore.'
  }

  $existingByName = @{}
  foreach ($policy in $existingPolicies) {
    $existingByName[[string]$policy.Name] = $policy
  }

  foreach ($spec in $desiredSpecs) {
    $existing = $existingByName[$spec.Name]
    if ($null -ne $existing -and (Test-UjQosSpecMatchesPolicy -Spec $spec -Policy $existing)) {
      continue
    }

    if (-not $PSCmdlet.ShouldProcess($spec.Name, 'Restore NetQosPolicy from backup')) {
      continue
    }

    $didWork = $true
    $removedExisting = $false
    try {
      if ($null -ne $existing) {
        Remove-NetQosPolicy -Name $spec.Name -Confirm:$false -ErrorAction Stop | Out-Null
        $removedExisting = $true
      }
      New-UjQosPolicyFromSpec -Spec $spec
    } catch {
      $hadFailure = $true
      Write-Warning -Message ("Could not restore network priority rule '{0}': {1}" -f $spec.Name, $_.Exception.Message)
      if ($removedExisting) {
        try {
          New-UjQosPolicyFromSpec -Spec (ConvertTo-UjQosBackupSpec -Item $existing)
        } catch {
          Write-Warning -Message ("Could not recover original network priority rule '{0}': {1}" -f $spec.Name, $_.Exception.Message)
        }
      }
    }
  }

  if (-not $hadFailure) {
    $desiredNames = @($desiredSpecs | ForEach-Object { $_.Name })
    foreach ($policy in $existingPolicies) {
      if ($policy.Name -in $desiredNames) {
        continue
      }
      if ($PSCmdlet.ShouldProcess($policy.Name, 'Remove stale managed NetQosPolicy')) {
        $didWork = $true
        try {
          Remove-NetQosPolicy -Name $policy.Name -Confirm:$false -ErrorAction Stop | Out-Null
        } catch {
          $hadFailure = $true
          Write-Warning -Message ("Could not remove stale network priority rule '{0}': {1}" -f $policy.Name, $_.Exception.Message)
        }
      }
    }
  }

  if (-not $didWork) {
    return Get-UjRestoreComponentResult -Status 'Skipped' -Message 'QoS restore skipped by ShouldProcess.'
  }

  if ($hadFailure) {
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
