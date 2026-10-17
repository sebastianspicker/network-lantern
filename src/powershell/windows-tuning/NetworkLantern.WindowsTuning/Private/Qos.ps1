function Get-NetworkTuningManagedQosPolicy {
  [CmdletBinding()]
  [OutputType('Microsoft.Management.Infrastructure.CimInstance')]
  param(
    [Parameter()]
    [switch]$ErrorOnFailure
  )

  try {
    Get-NetQosPolicy -ErrorAction Stop | Where-Object {
      $name = [string]$_.Name
      foreach ($prefix in $script:NetworkTuningManagedQosNamePrefixes) {
        if ($name.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
          return $true
        }
      }
      return $false
    }
  } catch {
    if ($ErrorOnFailure) {
      throw
    }
    Write-Warning -Message ("Could not read existing QoS policies. Proceeding as if none exist. ({0})" -f $_.Exception.Message)
    Write-Verbose -Message "QoS query failure - backup/restore may be incomplete. Error: $($_.Exception.Message)"
    return
  }
}

function Test-NetworkTuningQosPortPolicyNameMatch {
  [CmdletBinding()]
  [OutputType([bool])]
  param(
    [Parameter(Mandatory)][AllowEmptyString()][string]$PolicyName,
    [Parameter(Mandatory)][string]$BaseName
  )

  # A policy belongs to BaseName when it is exactly BaseName or BaseName_<port> (range members).
  # An unanchored prefix would also match ..._PORT_5000 against ..._PORT_50001.
  return ($PolicyName -match ('^' + [regex]::Escape($BaseName) + '(?:_\d+)?$'))
}

function New-NetworkTuningDscpPolicyByPort {
  [CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
  [OutputType([bool], [string])]
  param(
    [Parameter(Mandatory)]
    [string]$Name,

    [Parameter(Mandatory)]
    [uint16]$PortStart,

    [Parameter(Mandatory)]
    [uint16]$PortEnd,

    [Parameter()]
    [ValidateRange(0, 63)]
    [sbyte]$Dscp = $script:NetworkTuningDefaultDscp,

    [Parameter()]
    [switch]$DryRun
  )

  if ($PortEnd -lt $PortStart) {
    throw 'PortEnd must be >= PortStart.'
  }

  $portCount = [int]$PortEnd - [int]$PortStart + 1
  $maxIndividualPolicies = $script:NetworkTuningMaxPortPolicies

  if ($portCount -gt $maxIndividualPolicies) {
    Write-Warning -Message ("Your port range covers {0} ports, which is a lot. Windows may slow down with too many individual QoS rules." -f $portCount)
    Write-Warning -Message ("Only the first {0} ports will get individual rules. For broader coverage, use the 'Prioritize specific game/app .exe files' option instead." -f $maxIndividualPolicies)
    $effectivePortEnd = [int]$PortStart + $maxIndividualPolicies - 1
  } else {
    $effectivePortEnd = [int]$PortEnd
  }

  if ($DryRun) {
    $actualCount = $effectivePortEnd - [int]$PortStart + 1
    Write-NetworkTuningInformation -Message ("[DryRun] QoS {0} UDP {1}-{2} DSCP={3} ({4} individual policies)" -f $Name, $PortStart, $effectivePortEnd, $Dscp, $actualCount)
    return $true
  }

  # Clean up existing policies that match the prefix
  $existingPolicies = Get-NetworkTuningManagedQosPolicy -ErrorOnFailure | Where-Object { Test-NetworkTuningQosPortPolicyNameMatch -PolicyName $_.Name -BaseName $Name }
  foreach ($existing in $existingPolicies) {
    if ($PSCmdlet.ShouldProcess($existing.Name, 'Remove existing NetQosPolicy')) {
      try { Remove-NetQosPolicy -Name $existing.Name -Confirm:$false -ErrorAction Stop | Out-Null }
      catch { Write-Verbose -Message ("Failed to remove policy {0}: {1}" -f $existing.Name, $_.Exception.Message) }
    }
  }

  $createdCount = 0
  $declinedCount = 0
  for ($port = [int]$PortStart; $port -le $effectivePortEnd; $port++) {
    $policyName = if ($PortStart -eq $effectivePortEnd) { $Name } else { "{0}_{1}" -f $Name, $port }
    if (-not $PSCmdlet.ShouldProcess($policyName, 'Create NetQosPolicy (DSCP by UDP port)')) {
      $declinedCount++
      continue
    }

    try {
      New-NetQosPolicy -Name $policyName -IPPortMatchCondition ([uint16]$port) -IPProtocolMatchCondition UDP -DSCPAction $Dscp -NetworkProfile All -ErrorAction Stop | Out-Null
      $createdCount++
    } catch {
      Write-Warning -Message ("Could not create a network priority rule for port {0}. Windows may have hit its policy limit. ({1})" -f $port, $_.Exception.Message)
      return $false # Stop if we hit a system limit
    }
  }

  # 'Skipped' means every policy was declined and nothing was applied.
  if ($createdCount -eq 0 -and $declinedCount -gt 0) { return 'Skipped' }
  return $true
}

function New-NetworkTuningDscpPolicyByApp {
  [CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
  [OutputType([bool], [string])]
  param(
    [Parameter(Mandatory)]
    [string]$Name,

    [Parameter(Mandatory)]
    [string]$ExePath,

    [Parameter()]
    [ValidateRange(0, 63)]
    [sbyte]$Dscp = $script:NetworkTuningDefaultDscp,

    [Parameter()]
    [switch]$DryRun
  )

  if ($DryRun) {
    Write-NetworkTuningInformation -Message ("[DryRun] QoS {0} App={1} DSCP={2} (local store)" -f $Name, $ExePath, $Dscp)
    return $true
  }

  foreach ($existing in (Get-NetworkTuningManagedQosPolicy -ErrorOnFailure | Where-Object { $_.Name -eq $Name })) {
    if (-not $PSCmdlet.ShouldProcess($existing.Name, 'Remove NetQosPolicy')) {
      continue
    }

    try {
      Remove-NetQosPolicy -Name $existing.Name -Confirm:$false -ErrorAction Stop | Out-Null
    } catch {
      Write-Verbose -Message ("Failed to remove existing QoS policy: {0}" -f $existing.Name)
    }
  }

  if (-not $PSCmdlet.ShouldProcess($Name, 'Create NetQosPolicy (DSCP by app path)')) {
    return 'Skipped'
  }

  try {
    New-NetQosPolicy -Name $Name -AppPathNameMatchCondition $ExePath -DSCPAction $Dscp -NetworkProfile All -ErrorAction Stop | Out-Null
    return $true
  } catch {
    Write-Warning -Message ("Could not create a network priority rule for '{0}' ({1}). Check that the file path is correct. ({2})" -f $Name, $ExePath, $_.Exception.Message)
    return $false
  }
}
