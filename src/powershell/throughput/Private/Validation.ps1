# Validation helpers: hostname/IP and ping args (private to Iperf3TestSuite)

function Test-ValidHostnameOrIP {
  <#
  .SYNOPSIS
  Validates that a string is a valid hostname or IP address.
  .DESCRIPTION
  Performs strict validation to prevent argument injection attacks.
  .OUTPUTS
  [bool]
  #>
  [CmdletBinding()]
  [OutputType([bool])]
  param(
    [Parameter(Mandatory)]
    [string]$Name
  )

  if ([string]::IsNullOrWhiteSpace($Name)) {
    return $false
  }
  if ($Name -match '^-') {
    return $false
  }
  # Check IPv4 dotted-decimal BEFORE hostname regex so that out-of-range octets (e.g. 256.1.1.1) are rejected.
  if ($Name -match '^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$') {
    $octets = $Name.Split('.')
    foreach ($octet in $octets) {
      $val = [int]$octet
      if ($val -lt 0 -or $val -gt 255) { return $false }
    }
    return $true
  }
  if ($Name -match '^[a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?)*$') {
    return $true
  }
  $ipv6Pattern = '^\[?([0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}\]?$|^\[?([0-9a-fA-F]{1,4}:){1,7}:?\]?$|^\[?:(:[0-9a-fA-F]{1,4}){1,7}\]?$|^\[?([0-9a-fA-F]{1,4}:){1,6}:[0-9a-fA-F]{1,4}\]?$|^\[?([0-9a-fA-F]{1,4}:){1,5}(:[0-9a-fA-F]{1,4}){1,2}\]?$|^\[?([0-9a-fA-F]{1,4}:){1,4}(:[0-9a-fA-F]{1,4}){1,3}\]?$|^\[?([0-9a-fA-F]{1,4}:){1,3}(:[0-9a-fA-F]{1,4}){1,4}\]?$|^\[?([0-9a-fA-F]{1,4}:){1,2}(:[0-9a-fA-F]{1,4}){1,5}\]?$|^\[?[0-9a-fA-F]{1,4}:((:[0-9a-fA-F]{1,4}){1,6})\]?$|^\[?:((:[0-9a-fA-F]{1,4}){1,7}|:)\]?$|^\[?::(ffff(:0{1,4}){0,1}:){0,1}(\d{1,3}\.){3}\d{1,3}\]?$'
  if ($Name -match $ipv6Pattern) { return $true }
  # Fallback IPv6 check: require 2-8 groups of hex digits separated by colons (with optional brackets).
  if ($Name -match '^\[?[0-9a-fA-F:]+\]?$' -and $Name -replace '\[|\]', '' -match ':') {
    $cleanName = $Name -replace '\[|\]', ''
    $groups = @($cleanName -split ':' | Where-Object { $_ -ne '' })
    if ($cleanName -match '^[0-9a-fA-F:]+$' -and $groups.Count -ge 2 -and $groups.Count -le 8 -and $cleanName.Length -ge 2) {
      return $true
    }
  }
  return $false
}

function Get-PingArgumentsForStack {
  [CmdletBinding()]
  [OutputType([string])]
  param(
    [Parameter(Mandatory)]
    [ValidateSet('IPv4', 'IPv6')]
    [string]$Stack,
    [Parameter(Mandatory)]
    [string]$ComputerName,
    [int]$MtuPayloadSize = 0
  )
  $pingArgs = if ($Stack -eq 'IPv4') { @('-4', '-n', '1') } else { @('-6', '-n', '1') }
  if ($MtuPayloadSize -gt 0) {
    if ($Stack -eq 'IPv4') { $pingArgs += @('-f', '-l', "$MtuPayloadSize") }
    else { $pingArgs += @('-l', "$MtuPayloadSize") }
  }
  $pingArgs += $ComputerName
  return $pingArgs
}
