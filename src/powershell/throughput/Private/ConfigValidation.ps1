# Configuration/profile validation helpers (private to NetworkLantern.Throughput)

function ConvertTo-Iperf3IntArray {
  [CmdletBinding()]
  [OutputType([int[]])]
  param(
    [Parameter(Mandatory)]
    [object]$Value
  )
  $items = @($Value)
  $out = New-Object System.Collections.Generic.List[int]
  foreach ($item in $items) {
    try { [void]$out.Add([int]$item) }
    catch { throw "Expected integer array value but got '$item'." }
  }
  return $out.ToArray()
}

function ConvertTo-Iperf3StringArray {
  [CmdletBinding()]
  [OutputType([string[]])]
  param(
    [Parameter(Mandatory)]
    [object]$Value
  )
  $items = @($Value)
  $out = New-Object System.Collections.Generic.List[string]
  foreach ($item in $items) {
    $s = [string]$item
    if (-not [string]::IsNullOrWhiteSpace($s)) { [void]$out.Add($s.Trim()) }
  }
  return $out.ToArray()
}

function ConvertTo-Iperf3NonEmptyString {
  [CmdletBinding()]
  [OutputType([string])]
  param(
    [Parameter(Mandatory)]
    [object]$Value,
    [Parameter(Mandatory)]
    [string]$Key
  )
  $s = [string]$Value
  if ([string]::IsNullOrWhiteSpace($s)) {
    throw "Expected non-empty string for '$Key'."
  }
  return $s.Trim()
}

function ConvertTo-Iperf3IntInRange {
  [CmdletBinding()]
  [OutputType([int])]
  param(
    [Parameter(Mandatory)]
    [object]$Value,
    [Parameter(Mandatory)]
    [string]$Key,
    [Parameter(Mandatory)]
    [int]$Min,
    [Parameter(Mandatory)]
    [int]$Max
  )
  try {
    $n = [int]$Value
  }
  catch {
    throw "Expected integer value for '$Key' but got '$Value'."
  }
  if ($n -lt $Min -or $n -gt $Max) {
    throw "Value for '$Key' must be in range [$Min..$Max], got '$n'."
  }
  return $n
}

function ConvertTo-Iperf3DoubleInRange {
  [CmdletBinding()]
  [OutputType([double])]
  param(
    [Parameter(Mandatory)]
    [object]$Value,
    [Parameter(Mandatory)]
    [string]$Key,
    [Parameter(Mandatory)]
    [double]$Min,
    [Parameter(Mandatory)]
    [double]$Max
  )
  try {
    $n = [double]$Value
  }
  catch {
    throw "Expected numeric value for '$Key' but got '$Value'."
  }
  if ($n -lt $Min -or $n -gt $Max) {
    throw "Value for '$Key' must be in range [$Min..$Max], got '$n'."
  }
  return $n
}

function ConvertTo-Iperf3Bool {
  [CmdletBinding()]
  [OutputType([bool])]
  param(
    [Parameter(Mandatory)]
    [object]$Value
  )
  if ($Value -is [bool]) { return [bool]$Value }
  $s = [string]$Value
  if ($s -match '^(?i:true|1|yes|on)$') { return $true }
  if ($s -match '^(?i:false|0|no|off)$') { return $false }
  throw "Expected boolean value but got '$Value'."
}

function ConvertTo-Iperf3KnownValue {
  [CmdletBinding()]
  param(
    [Parameter(Mandatory)]
    [string]$Key,
    [Parameter(Mandatory)]
    [object]$Value
  )
  switch ($Key) {
    'Target' { return (ConvertTo-Iperf3NonEmptyString -Value $Value -Key 'Target') }
    'OutDir' {
      $s = ConvertTo-Iperf3NonEmptyString -Value $Value -Key 'OutDir'
      if ($s -match '[\x00-\x1f]') { throw "OutDir path contains control characters." }
      if ($s.StartsWith('-')) { throw "OutDir path must not look like an option (starts with '-'): $s" }
      return $s
    }
    'ProfileName' { return (ConvertTo-Iperf3NonEmptyString -Value $Value -Key 'ProfileName') }
    'ProfilesFile' { return (ConvertTo-Iperf3NonEmptyString -Value $Value -Key 'ProfilesFile') }
    'UdpStart' { return (ConvertTo-Iperf3NonEmptyString -Value $Value -Key 'UdpStart') }
    'UdpMax' { return (ConvertTo-Iperf3NonEmptyString -Value $Value -Key 'UdpMax') }
    'UdpStep' { return (ConvertTo-Iperf3NonEmptyString -Value $Value -Key 'UdpStep') }
    'Port' { return (ConvertTo-Iperf3IntInRange -Value $Value -Key 'Port' -Min 1 -Max 65535) }
    'Duration' { return (ConvertTo-Iperf3IntInRange -Value $Value -Key 'Duration' -Min 1 -Max 3600) }
    'Omit' { return (ConvertTo-Iperf3IntInRange -Value $Value -Key 'Omit' -Min 0 -Max 60) }
    'ConnectTimeoutMs' { return (ConvertTo-Iperf3IntInRange -Value $Value -Key 'ConnectTimeoutMs' -Min 1000 -Max 300000) }
    'RetryCount' { return (ConvertTo-Iperf3IntInRange -Value $Value -Key 'RetryCount' -Min 0 -Max 5) }
    'ThresholdMinThroughputMbps' { return (ConvertTo-Iperf3DoubleInRange -Value $Value -Key 'ThresholdMinThroughputMbps' -Min 0 -Max 1000000) }
    'ThresholdMaxLossPct' { return (ConvertTo-Iperf3DoubleInRange -Value $Value -Key 'ThresholdMaxLossPct' -Min 0 -Max 100) }
    'ThresholdMaxJitterMs' { return (ConvertTo-Iperf3DoubleInRange -Value $Value -Key 'ThresholdMaxJitterMs' -Min 0 -Max 1000000) }
    'UdpLossThreshold' { return (ConvertTo-Iperf3DoubleInRange -Value $Value -Key 'UdpLossThreshold' -Min 0 -Max 100) }
    'MtuSizes' {
      $arr = @(ConvertTo-Iperf3IntArray -Value $Value)
      if ($arr.Count -eq 0) { throw "Expected non-empty integer array for 'MtuSizes'." }
      foreach ($n in $arr) {
        if ($n -lt 1 -or $n -gt 65500) { throw "Value '$n' in 'MtuSizes' must be in range [1..65500]." }
      }
      return $arr
    }
    'TcpStreams' {
      $arr = @(ConvertTo-Iperf3IntArray -Value $Value)
      if ($arr.Count -eq 0) { throw "Expected non-empty integer array for 'TcpStreams'." }
      foreach ($n in $arr) {
        if ($n -lt 1 -or $n -gt 128) { throw "Value '$n' in 'TcpStreams' must be in range [1..128]." }
      }
      return $arr
    }
    'TcpWindows' {
      $arr = @(ConvertTo-Iperf3StringArray -Value $Value)
      if ($arr.Count -eq 0) { throw "Expected non-empty string array for 'TcpWindows'." }
      foreach ($w in $arr) {
        if ($w -notmatch '^(default|[0-9]+[kKmMgG]?)$') {
          throw "Invalid TCP window size '$w' in 'TcpWindows'. Expected 'default' or a number with optional K/M/G suffix."
        }
      }
      return $arr
    }
    'DscpClasses' {
      $arr = @(ConvertTo-Iperf3StringArray -Value $Value)
      if ($arr.Count -eq 0) { throw "Expected non-empty string array for 'DscpClasses'." }
      foreach ($d in $arr) {
        if ($d -cnotmatch '^(CS[0-7]|EF|AF[1-4][1-3])$') {
          throw "Invalid DSCP class '$d' in 'DscpClasses'. Expected CS0-CS7, EF, or AFxy (e.g. AF11, AF41)."
        }
      }
      return $arr
    }
    'Quiet' { return (ConvertTo-Iperf3Bool -Value $Value) }
    'Progress' { return (ConvertTo-Iperf3Bool -Value $Value) }
    'Summary' { return (ConvertTo-Iperf3Bool -Value $Value) }
    'DisableMtuProbe' { return (ConvertTo-Iperf3Bool -Value $Value) }
    'SkipReachabilityCheck' { return (ConvertTo-Iperf3Bool -Value $Value) }
    'Force' { return (ConvertTo-Iperf3Bool -Value $Value) }
    'WhatIf' { return (ConvertTo-Iperf3Bool -Value $Value) }
    'SingleTest' { return (ConvertTo-Iperf3Bool -Value $Value) }
    'ListProfiles' { return (ConvertTo-Iperf3Bool -Value $Value) }
    'SaveProfile' { return (ConvertTo-Iperf3Bool -Value $Value) }
    'StrictConfiguration' { return (ConvertTo-Iperf3Bool -Value $Value) }
    'PassThru' { return (ConvertTo-Iperf3Bool -Value $Value) }
    'IpVersion' {
      $v = [string]$Value
      if ($v -notin @('Auto', 'IPv4', 'IPv6')) { throw "Invalid IpVersion '$v'." }
      return $v
    }
    'Protocol' {
      $v = [string]$Value
      if ($v -notin @('TCP', 'UDP', 'Both')) { throw "Invalid Protocol '$v'." }
      return $v
    }
    default { throw "Unknown parameter key '$Key'. Run Get-NetworkThroughputDefaultParameterSet to see valid parameter names." }
  }
}

function ConvertTo-Iperf3NormalizedParameterSet {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]
    [hashtable]$InputParameters,
    [Parameter(Mandatory)]
    [string[]]$AllowedKeys,
    [switch]$StrictConfiguration
  )
  $normalized = @{}
  $warnings = New-Object System.Collections.Generic.List[string]
  foreach ($key in $InputParameters.Keys) {
    if ($key -notin $AllowedKeys) {
      $msg = "Unknown configuration key '$key' ignored."
      if ($StrictConfiguration) { throw $msg }
      [void]$warnings.Add($msg)
      continue
    }
    try {
      $normalized[$key] = ConvertTo-Iperf3KnownValue -Key $key -Value $InputParameters[$key]
    }
    catch {
      $msg = "Invalid value for key '$key': $($_.Exception.Message)"
      if ($StrictConfiguration) { throw $msg }
      [void]$warnings.Add($msg)
    }
  }
  [pscustomobject]@{
    Parameters = $normalized
    Warnings   = $warnings.ToArray()
  }
}
