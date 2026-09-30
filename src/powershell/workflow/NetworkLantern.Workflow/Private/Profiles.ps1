$script:MaxWorkflowProfileBytes = 1MB
$script:MaxWorkflowProfileReadBytes = $script:MaxWorkflowProfileBytes + 1
$script:MaxWorkflowProfileJsonDepth = 16

function Read-NetworkLanternWorkflowProfileText {
  [CmdletBinding()]
  param([Parameter(Mandatory)][System.IO.Stream]$Stream)

  $buffer = [byte[]]::new($script:MaxWorkflowProfileReadBytes)
  $bytesRead = 0
  while ($bytesRead -lt $buffer.Length) {
    $readCount = $Stream.Read($buffer, $bytesRead, $buffer.Length - $bytesRead)
    if ($readCount -eq 0) {
      break
    }
    $bytesRead += $readCount
  }

  if ($bytesRead -gt $script:MaxWorkflowProfileBytes) {
    throw 'ProfilePath exceeds maximum size (1 MB).'
  }

  return [System.Text.UTF8Encoding]::new($false, $true).GetString($buffer, 0, $bytesRead)
}

function Import-NetworkLanternWorkflowProfile {
  [CmdletBinding()]
  param([string]$ProfilePath)

  if ([string]::IsNullOrWhiteSpace($ProfilePath)) {
    return @{}
  }

  $resolvedPath = if ([System.IO.Path]::IsPathRooted($ProfilePath)) {
    [System.IO.Path]::GetFullPath($ProfilePath)
  } else {
    [System.IO.Path]::GetFullPath((Join-Path (Get-Location) $ProfilePath))
  }
  try {
    $stream = [System.IO.File]::Open(
      $resolvedPath,
      [System.IO.FileMode]::Open,
      [System.IO.FileAccess]::Read,
      [System.IO.FileShare]::Read
    )
  } catch [System.IO.FileNotFoundException] {
    throw "ProfilePath is not a file: $resolvedPath"
  } catch [System.IO.DirectoryNotFoundException] {
    throw "ProfilePath is not a file: $resolvedPath"
  } catch [System.UnauthorizedAccessException] {
    throw "ProfilePath is not a file: $resolvedPath"
  }
  try {
    $profileText = Read-NetworkLanternWorkflowProfileText -Stream $stream
  } finally {
    $stream.Dispose()
  }

  $parsed = $profileText | ConvertFrom-Json -AsHashtable -Depth $script:MaxWorkflowProfileJsonDepth -ErrorAction Stop
  if ($null -eq $parsed) {
    return @{}
  }

  return $parsed
}

function Get-NetworkLanternProfileSection {
  [CmdletBinding()]
  param(
    [Parameter(Mandatory)][hashtable]$ProfileMap,
    [Parameter(Mandatory)][string]$Name
  )

  if ($ProfileMap.ContainsKey($Name) -and $ProfileMap[$Name] -is [hashtable]) {
    return $ProfileMap[$Name]
  }

  return @{}
}

function Write-NetworkLanternWorkflowProfileWarnings {
  [CmdletBinding()]
  param([Parameter(Mandatory)][hashtable]$ProfileMap)

  $knownKeysBySection = @{
    path          = @('hostsIPv4', 'hostsIPv6', 'protocols', 'rounds')
    throughput    = @('target', 'port', 'protocol', 'maxTotalTests')
    windowsTuning = @('action', 'profile', 'udpPorts', 'appPaths')
  }

  foreach ($sectionName in @($ProfileMap.Keys)) {
    if (-not $knownKeysBySection.ContainsKey($sectionName)) {
      Write-Warning -Message "Unknown workflow profile section '$sectionName' will be ignored."
      continue
    }

    $section = $ProfileMap[$sectionName]
    if ($section -isnot [hashtable]) {
      Write-Warning -Message "Workflow profile section '$sectionName' is not an object and will be ignored."
      continue
    }

    foreach ($key in @($section.Keys)) {
      if ($key -notin @($knownKeysBySection[$sectionName])) {
        Write-Warning -Message "Unknown workflow profile key '$sectionName.$key' will be ignored."
      }
    }
  }
}

function ConvertTo-NetworkLanternWorkflowString {
  [CmdletBinding()]
  param(
    [AllowNull()][object]$Value,
    [Parameter(Mandatory)][string]$Name
  )

  if ($null -eq $Value) { return $null }
  if ($Value -isnot [string]) {
    throw "$Name must be a string."
  }
  return [string]$Value
}

function ConvertTo-NetworkLanternWorkflowStringArray {
  [CmdletBinding()]
  param(
    [AllowNull()][object]$Value,
    [Parameter(Mandatory)][string]$Name,
    [string[]]$AllowedValues
  )

  if ($null -eq $Value) { return [string[]]@() }
  $items = if ($Value -is [string]) {
    @($Value)
  } elseif ($Value -is [System.Collections.IEnumerable] -and $Value -isnot [System.Collections.IDictionary]) {
    @($Value)
  } else {
    throw "$Name must be a string or an array of strings."
  }

  $result = foreach ($item in $items) {
    if ($item -isnot [string]) {
      throw "$Name must contain only strings."
    }
    if ($PSBoundParameters.ContainsKey('AllowedValues') -and $item -notin $AllowedValues) {
      throw "$Name contains unsupported value '$item'. Allowed values: $($AllowedValues -join ', ')."
    }
    [string]$item
  }
  return [string[]]@($result)
}

function ConvertTo-NetworkLanternWorkflowInteger {
  [CmdletBinding()]
  param(
    [AllowNull()][object]$Value,
    [Parameter(Mandatory)][string]$Name,
    [Parameter(Mandatory)][long]$Minimum,
    [Parameter(Mandatory)][long]$Maximum
  )

  $integerTypes = @([byte], [sbyte], [int16], [uint16], [int32], [uint32], [int64], [uint64])
  if ($null -eq $Value -or $Value.GetType() -notin $integerTypes) {
    throw "$Name must be an integer."
  }
  try {
    $number = [long]$Value
  } catch {
    throw "$Name must be between $Minimum and $Maximum."
  }
  if ($number -lt $Minimum -or $number -gt $Maximum) {
    throw "$Name must be between $Minimum and $Maximum."
  }
  return $number
}

function ConvertTo-NetworkLanternWorkflowUInt16Array {
  [CmdletBinding()]
  param(
    [AllowNull()][object]$Value,
    [Parameter(Mandatory)][string]$Name
  )

  if ($null -eq $Value) { return [uint16[]]@() }
  $items = if ($Value -is [System.Collections.IEnumerable] -and
      $Value -isnot [string] -and $Value -isnot [System.Collections.IDictionary]) {
    @($Value)
  } else {
    @($Value)
  }
  $result = foreach ($item in $items) {
    [uint16](ConvertTo-NetworkLanternWorkflowInteger -Value $item -Name $Name -Minimum 0 -Maximum 65535)
  }
  return [uint16[]]@($result)
}

function Resolve-NetworkLanternEffectiveValue {
  [CmdletBinding()]
  param(
    [object]$ExplicitValue,
    [Parameter(Mandatory)][bool]$ExplicitValueWasProvided,
    [Parameter(Mandatory)][hashtable]$Section,
    [Parameter(Mandatory)][string]$Key,
    [object]$Fallback = $null
  )

  if ($ExplicitValueWasProvided -and $null -ne $ExplicitValue) {
    if ($ExplicitValue -is [array] -and $ExplicitValue.Count -eq 0) {
      if ($Section.ContainsKey($Key)) { return $Section[$Key] }
      return $Fallback
    }

    if ($ExplicitValue -is [string] -and [string]::IsNullOrWhiteSpace($ExplicitValue)) {
      if ($Section.ContainsKey($Key)) { return $Section[$Key] }
      return $Fallback
    }

    return $ExplicitValue
  }

  if ($Section.ContainsKey($Key)) {
    return $Section[$Key]
  }

  return $Fallback
}
