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

  foreach ($artifactName in $expectedNames) {
    $artifactPath = Join-Path -Path $BackupFolder -ChildPath $artifactName
    if (-not (Test-Path -LiteralPath $artifactPath -PathType Leaf)) {
      return [pscustomobject]@{ IsValid = $false; Message = "Expected backup artifact is missing: $artifactName" }
    }
    if (-not $digests.Contains($artifactName)) {
      return [pscustomobject]@{ IsValid = $false; Message = "Expected backup artifact has no manifest digest: $artifactName" }
    }
  }

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

