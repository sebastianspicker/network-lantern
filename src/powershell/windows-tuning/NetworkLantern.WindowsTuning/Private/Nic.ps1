function Get-NetworkTuningPhysicalUpAdapter {
  [CmdletBinding()]
  [OutputType([Microsoft.PowerShell.Cmdletization.GeneratedTypes.NetAdapter.NetAdapter[]])]
  param()

  Get-NetAdapter -Physical -ErrorAction Stop | Where-Object { $_.Status -eq 'Up' }
}

function Resolve-NetworkTuningNicRegistryValue {
  <#
  .SYNOPSIS
    Map a display value (for example 'Disabled') to the raw registry data the driver expects.
  .OUTPUTS
    [string] raw registry value, or $null when the property does not list the display value.
  #>
  [CmdletBinding()]
  [OutputType([string])]
  param(
    [Parameter(Mandatory)]$Property,
    [Parameter(Mandatory)][string]$DisplayValue
  )

  $displayValues = @()
  $registryValues = @()
  if ($Property.PSObject.Properties['ValidDisplayValues']) { $displayValues = @($Property.ValidDisplayValues) }
  if ($Property.PSObject.Properties['ValidRegistryValues']) { $registryValues = @($Property.ValidRegistryValues) }
  for ($i = 0; $i -lt $displayValues.Count; $i++) {
    if ([string]::Equals([string]$displayValues[$i], $DisplayValue, [System.StringComparison]::OrdinalIgnoreCase)) {
      if ($i -lt $registryValues.Count) { return [string]$registryValues[$i] }
    }
  }
  return $null
}

function Set-NetworkTuningNicAdvancedPropertyIfSupported {
  [CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
  [OutputType([bool], [string])]
  param(
    [Parameter(Mandatory)]
    [string]$Name,

    [Parameter(Mandatory)]
    [string]$DisplayName,

    [Parameter(Mandatory)]
    [string]$Value,

    # Adapter|keyword (or Adapter|DisplayName) pairs Restore can undo; $null skips the check.
    [Parameter()]
    [System.Collections.Generic.HashSet[string]]$BackedUpProperty,

    [Parameter()]
    [switch]$DryRun
  )

  # Prefer standardized RegistryKeywords over localized DisplayNames
  $keyword = if ($script:NetworkTuningNicKeywordMap.ContainsKey($DisplayName)) { $script:NetworkTuningNicKeywordMap[$DisplayName] } else { $null }

  $property = if ($keyword) {
    Get-NetAdapterAdvancedProperty -Name $Name -RegistryKeyword $keyword -ErrorAction SilentlyContinue
  } else {
    Get-NetAdapterAdvancedProperty -Name $Name -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -eq $DisplayName }
  }

  if (-not $property) {
    Write-Verbose -Message ("{0}: property '{1}' (keyword={2}) not found or not supported." -f $Name, $DisplayName, $keyword)
    return $true
  }

  $backupKey = "{0}|{1}" -f $Name, $(if ($keyword) { $keyword } else { $DisplayName })
  if ($null -ne $BackedUpProperty -and -not $BackedUpProperty.Contains($backupKey)) {
    Write-Warning -Message ("Network adapter '{0}': '{1}' was not changed because the backup holds no restorable value for it." -f $Name, $DisplayName)
    return $true
  }

  if ($DryRun) {
    $keywordLabel = if ($keyword) { $keyword } else { 'no-keyword' }
    Write-NetworkTuningInformation -Message ("[DryRun] {0}: {1} ({2}) => {3}" -f $Name, $DisplayName, $keywordLabel, $Value)
    return $true
  }

  $targetHint = if ($keyword) { "Keyword: $keyword" } else { "DisplayName: $DisplayName" }
  if (-not $PSCmdlet.ShouldProcess(("{0}: {1}" -f $Name, $DisplayName), ("Set to '{0}' via {1}" -f $Value, $targetHint))) {
    return 'Skipped'
  }

  try {
    if ($keyword) {
      # $Value is a display value; RegistryValue needs the matching raw registry data.
      $registryValue = Resolve-NetworkTuningNicRegistryValue -Property @($property)[0] -DisplayValue $Value
      if ($null -eq $registryValue) {
        Write-Warning -Message ("Network adapter '{0}': '{1}' does not offer the value '{2}', so it was not changed." -f $Name, $DisplayName, $Value)
        return $false
      }
      Set-NetAdapterAdvancedProperty -Name $Name -RegistryKeyword $keyword -RegistryValue $registryValue -NoRestart -ErrorAction Stop | Out-Null
    } else {
      Set-NetAdapterAdvancedProperty -Name $Name -DisplayName $DisplayName -DisplayValue $Value -NoRestart -ErrorAction Stop | Out-Null
    }
    return $true
  } catch {
    Write-Warning -Message ("Network adapter '{0}': could not change '{1}'. Your adapter or driver may not support this setting. ({2})" -f $Name, $DisplayName, $_.Exception.Message)
    return $false
  }
}

function Set-NetworkTuningNicConfiguration {
  [CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
  [OutputType([bool], [string])]
  param(
    [Parameter()]
    [string]$BackupFolder,

    [Parameter()]
    [switch]$DryRun
  )

  $keywords = [System.Collections.Generic.List[string]]::new()
  $keywords.AddRange([string[]]$script:NetworkTuningNicKeywordsTier1)

  try {
    $adapters = @(Get-NetworkTuningPhysicalUpAdapter)
  } catch {
    Write-Warning -Message ("Could not detect your network adapters. Make sure you have an active Ethernet connection. ({0})" -f $_.Exception.Message)
    return $false
  }

  if ($adapters.Count -eq 0) {
    Write-Warning -Message 'No active physical network adapter was found.'
    return $false
  }

  # Change only properties the backup recorded, so Restore can undo every change.
  $backedUpProperty = $null
  if (-not $DryRun -and -not [string]::IsNullOrWhiteSpace($BackupFolder)) {
    $backedUpProperty = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    $csv = Join-Path -Path $BackupFolder -ChildPath $script:NetworkTuningBackupFileNicAdvanced
    if (Test-Path -LiteralPath $csv -PathType Leaf) {
      try {
        foreach ($row in @(Read-NetworkTuningBoundedTextFile -Path $csv -MaximumBytes $script:NetworkTuningMaxCsvBackupBytes | ConvertFrom-Csv -ErrorAction Stop)) {
          # A keyword row counts only with a usable registry value; rows without a keyword count by DisplayName.
          # Stricter than Restore, which can fall back to DisplayName: Apply changes less, never more.
          if (-not [string]::IsNullOrEmpty($row.RegistryKeyword)) {
            if (-not [string]::IsNullOrEmpty($row.RegistryValue) -and [string]$row.RegistryValue -cne 'System.String[]') {
              $backedUpProperty.Add(("{0}|{1}" -f $row.Adapter, $row.RegistryKeyword)) | Out-Null
            }
          } elseif (-not [string]::IsNullOrEmpty($row.DisplayName)) {
            $backedUpProperty.Add(("{0}|{1}" -f $row.Adapter, $row.DisplayName)) | Out-Null
          }
        }
      } catch {
        Write-Warning -Message ("Could not read the network adapter backup, so no adapter settings were changed. ({0})" -f $_.Exception.Message)
        return $false
      }
    }
  }

  $allSucceeded = $true
  $appliedCount = 0
  $declinedCount = 0
  foreach ($nic in $adapters) {
    Write-NetworkTuningInformation -Message ("NIC: {0}" -f $nic.Name)

    foreach ($keyword in $keywords) {
      $displayName = if ($script:NetworkTuningNicKeywordReverseMap.ContainsKey($keyword)) { $script:NetworkTuningNicKeywordReverseMap[$keyword] } else { $keyword }
      $value = 'Disabled'
      $propertySucceeded = Set-NetworkTuningNicAdvancedPropertyIfSupported -Name $nic.Name -DisplayName $displayName -Value $value -BackedUpProperty $backedUpProperty -DryRun:$DryRun
      if ($false -eq $propertySucceeded) { $allSucceeded = $false }
      elseif (Test-NetworkTuningActionDeclined -Result $propertySucceeded) { $declinedCount++ }
      else { $appliedCount++ }
    }

  }

  # 'Skipped' means every change was declined and nothing was applied.
  if ($allSucceeded -and $appliedCount -eq 0 -and $declinedCount -gt 0 -and -not $DryRun) { return 'Skipped' }
  return $allSucceeded
}
