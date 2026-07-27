function Get-UjPhysicalUpAdapter {
  [CmdletBinding()]
  [OutputType([Microsoft.PowerShell.Cmdletization.GeneratedTypes.NetAdapter.NetAdapter[]])]
  param()

  Get-NetAdapter -Physical -ErrorAction Stop | Where-Object { $_.Status -eq 'Up' }
}

function Set-UjNicAdvancedPropertyIfSupported {
  [CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
  [OutputType([bool])]
  param(
    [Parameter(Mandatory)]
    [string]$Name,

    [Parameter(Mandatory)]
    [string]$DisplayName,

    [Parameter(Mandatory)]
    [string]$Value,

    [Parameter()]
    [switch]$DryRun
  )

  # Prefer standardized RegistryKeywords over localized DisplayNames
  $keyword = if ($script:UjNicKeywordMap.ContainsKey($DisplayName)) { $script:UjNicKeywordMap[$DisplayName] } else { $null }

  $property = if ($keyword) {
    Get-NetAdapterAdvancedProperty -Name $Name -RegistryKeyword $keyword -ErrorAction SilentlyContinue
  } else {
    Get-NetAdapterAdvancedProperty -Name $Name -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -eq $DisplayName }
  }

  if (-not $property) {
    Write-Verbose -Message ("{0}: property '{1}' (keyword={2}) not found or not supported." -f $Name, $DisplayName, $keyword)
    return $true
  }

  if ($DryRun) {
    $keywordLabel = if ($keyword) { $keyword } else { 'no-keyword' }
    Write-UjInformation -Message ("[DryRun] {0}: {1} ({2}) => {3}" -f $Name, $DisplayName, $keywordLabel, $Value)
    return $true
  }

  $targetHint = if ($keyword) { "Keyword: $keyword" } else { "DisplayName: $DisplayName" }
  if (-not $PSCmdlet.ShouldProcess(("{0}: {1}" -f $Name, $DisplayName), ("Set to '{0}' via {1}" -f $Value, $targetHint))) {
    return $true
  }

  try {
    if ($keyword) {
      Set-NetAdapterAdvancedProperty -Name $Name -RegistryKeyword $keyword -RegistryValue $Value -NoRestart -ErrorAction Stop | Out-Null
    } else {
      Set-NetAdapterAdvancedProperty -Name $Name -DisplayName $DisplayName -DisplayValue $Value -NoRestart -ErrorAction Stop | Out-Null
    }
    return $true
  } catch {
    Write-Warning -Message ("Network adapter '{0}': could not change '{1}'. Your adapter or driver may not support this setting. ({2})" -f $Name, $DisplayName, $_.Exception.Message)
    return $false
  }
}

function Set-UjNicConfiguration {
  [CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
  [OutputType([bool])]
  param(
    [Parameter(Mandatory)]
    [ValidateSet(1, 2, 3)]
    [int]$Preset,

    [Parameter()]
    [switch]$IncludeExperimental,

    [Parameter()]
    [switch]$DryRun
  )

  # Build list of keywords to apply based on preset tier
  $keywords = [System.Collections.Generic.List[string]]::new()
  $keywords.AddRange([string[]]$script:UjNicKeywordsTier1)
  if ($Preset -ge 2) { $keywords.AddRange([string[]]$script:UjNicKeywordsTier2) }
  if ($Preset -ge 3) { $keywords.AddRange([string[]]$script:UjNicKeywordsTier3) }
  if ($IncludeExperimental) { $keywords.AddRange([string[]]$script:UjNicKeywordsExperimental) }

  # Special-case values: most keywords get 'Disabled', but some need specific values
  $specialValues = @{
    '*InterruptModerationRate' = '0'
    '*ReceiveBuffers'         = '256'
    '*TransmitBuffers'        = '256'
  }

  try {
    $adapters = @(Get-UjPhysicalUpAdapter)
  } catch {
    Write-Warning -Message ("Could not detect your network adapters. Make sure you have an active Ethernet connection. ({0})" -f $_.Exception.Message)
    return $false
  }

  if ($adapters.Count -eq 0) {
    Write-Warning -Message 'No active physical network adapter was found.'
    return $false
  }

  $allSucceeded = $true
  foreach ($nic in $adapters) {
    Write-UjInformation -Message ("NIC: {0}" -f $nic.Name)

    foreach ($keyword in $keywords) {
      $displayName = if ($script:UjNicKeywordReverseMap.ContainsKey($keyword)) { $script:UjNicKeywordReverseMap[$keyword] } else { $keyword }
      $value = if ($specialValues.ContainsKey($keyword)) { $specialValues[$keyword] } else { 'Disabled' }
      $propertySucceeded = Set-UjNicAdvancedPropertyIfSupported -Name $nic.Name -DisplayName $displayName -Value $value -DryRun:$DryRun
      if ($false -eq $propertySucceeded) { $allSucceeded = $false }
    }

    # RSC disable: only with -IncludeExperimental (RSC is TCP-only coalescing)
    if ($IncludeExperimental) {
      if ($DryRun) {
        Write-UjInformation -Message ("[DryRun] Disable-NetAdapterRsc {0}" -f $nic.Name)
      } elseif ($PSCmdlet.ShouldProcess($nic.Name, 'Disable NetAdapterRsc')) {
        try {
          Disable-NetAdapterRsc -Name $nic.Name -Confirm:$false -ErrorAction Stop | Out-Null
        } catch {
          Write-Warning -Message ("Could not disable RSC (Receive Segment Coalescing) on adapter '{0}'. This is non-critical and can be ignored." -f $nic.Name)
        }
      }
    }
  }

  return $allSucceeded
}
