function New-NetworkTuningRestoreStagingSession {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([pscustomobject])]
  param()

  $isWindowsRuntime = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)
  $parentPath = if ($isWindowsRuntime) { Get-NetworkTuningWindowsRestoreStagingRoot } else { [System.IO.Path]::GetTempPath() }
  $stagingPath = Join-Path -Path $parentPath -ChildPath ("{0}{1}" -f $script:NetworkTuningRestoreStagingLeafPrefix, [guid]::NewGuid().ToString('N'))
  if (-not $PSCmdlet.ShouldProcess($stagingPath, 'Create restricted restore staging session')) {
    throw 'Restore staging session creation was declined.'
  }

  $sentinelStream = $null
  try {
    if ($isWindowsRuntime) {
      $stagingPath = Initialize-NetworkTuningAdminOnlyDirectory -Path $stagingPath -Confirm:$false
    } else {
      [System.IO.Directory]::CreateDirectory($stagingPath) | Out-Null
      $stagingItem = Get-Item -LiteralPath $stagingPath -Force -ErrorAction Stop
      if (($stagingItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw 'Restore staging session unexpectedly resolved to a reparse point.'
      }
    }

    $nonce = [guid]::NewGuid().ToString('N')
    $sentinelPath = Join-Path -Path $stagingPath -ChildPath $script:NetworkTuningRestoreStagingMarkerName
    $sentinelStream = [System.IO.File]::Open(
      $sentinelPath,
      [System.IO.FileMode]::CreateNew,
      [System.IO.FileAccess]::ReadWrite,
      [System.IO.FileShare]::Read
    )
    $nonceBytes = [System.Text.Encoding]::UTF8.GetBytes($nonce)
    $sentinelStream.Write($nonceBytes, 0, $nonceBytes.Length)
    $sentinelStream.Flush($true)
    $sentinelStream.Position = 0
    if ($isWindowsRuntime) {
      Protect-NetworkTuningAdminOnlyFile -Path $sentinelPath -Confirm:$false
    }

    return [pscustomobject]@{
      Path = $stagingPath
      ParentPath = $parentPath
      SentinelPath = $sentinelPath
      Nonce = $nonce
      SentinelStream = $sentinelStream
      IsWindows = $isWindowsRuntime
    }
  } catch {
    if ($null -ne $sentinelStream) { $sentinelStream.Dispose() }
    if (Test-Path -LiteralPath (Join-Path -Path $stagingPath -ChildPath $script:NetworkTuningRestoreStagingMarkerName) -PathType Leaf) {
      Remove-NetworkTuningRestoreStagingDirectory -Path $stagingPath -WindowsRuntime $isWindowsRuntime
    } else {
      # Creation failed before the marker existed: the full guard would refuse, which would leak
      # the (at most empty) directory. Remove it only if it is empty.
      Remove-NetworkTuningEmptyRestoreStagingDirectory -Path $stagingPath -WindowsRuntime $isWindowsRuntime
    }
    throw "Could not create a trusted restore staging session: $($_.Exception.Message)"
  }
}

function Get-NetworkTuningRestoreStagingAllowedRoot {
  [CmdletBinding()]
  [OutputType([string])]
  param([Parameter(Mandatory)][bool]$WindowsRuntime)

  if (-not $WindowsRuntime) { return [System.IO.Path]::GetTempPath() }
  $commonData = [System.Environment]::GetFolderPath([System.Environment+SpecialFolder]::CommonApplicationData)
  return Join-Path -Path (Join-Path -Path $commonData -ChildPath 'NetworkLantern-Privileged') -ChildPath 'RestoreStaging'
}

function Test-NetworkTuningRestoreStagingPath {
  <#
  .SYNOPSIS
    Pure allowlist guard: may this path be deleted as a restore staging directory?
  .DESCRIPTION
    Accepts only a fully qualified path that is a direct child of AllowedRoot, whose leaf
    matches the staging naming pattern, and which contains the staging marker file
    (reported by the caller through MarkerPresent). Performs no filesystem access.
  #>
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [AllowNull()][AllowEmptyString()][string]$Path,
    [AllowNull()][AllowEmptyString()][string]$AllowedRoot,
    [Parameter(Mandatory)][bool]$MarkerPresent,
    [Parameter(Mandatory)][bool]$WindowsRuntime
  )

  $deny = { param($message) [pscustomobject]@{ IsAllowed = $false; Message = $message } }
  if ([string]::IsNullOrWhiteSpace($Path) -or [string]::IsNullOrWhiteSpace($AllowedRoot)) {
    return (& $deny 'Staging path and allowed root must not be empty.')
  }
  if (-not [System.IO.Path]::IsPathFullyQualified($Path) -or -not [System.IO.Path]::IsPathFullyQualified($AllowedRoot)) {
    return (& $deny 'Staging path and allowed root must be fully qualified.')
  }

  $separators = [char[]]@([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
  $fullPath = [System.IO.Path]::GetFullPath($Path).TrimEnd($separators)
  $fullRoot = [System.IO.Path]::GetFullPath($AllowedRoot).TrimEnd($separators)
  if ([string]::IsNullOrEmpty($fullRoot) -or [string]::IsNullOrEmpty($fullPath)) {
    return (& $deny 'Staging path and allowed root must not be a filesystem root.')
  }

  $comparison = if ($WindowsRuntime) { [System.StringComparison]::OrdinalIgnoreCase } else { [System.StringComparison]::Ordinal }
  $parent = [System.IO.Path]::GetDirectoryName($fullPath)
  if ([string]::IsNullOrEmpty($parent) -or -not $parent.TrimEnd($separators).Equals($fullRoot, $comparison)) {
    return (& $deny "Staging path is not directly under the allowed staging root: $fullPath")
  }
  $leaf = [System.IO.Path]::GetFileName($fullPath)
  if ($leaf -cnotmatch $script:NetworkTuningRestoreStagingLeafPattern) {
    return (& $deny "Staging directory name does not match the staging pattern: $leaf")
  }
  if (-not $MarkerPresent) {
    return (& $deny "Staging directory does not contain the staging marker file: $fullPath")
  }
  return [pscustomobject]@{ IsAllowed = $true; Message = '' }
}

function Test-NetworkTuningRestoreStagingEmptyRemoval {
  <#
  .SYNOPSIS
    Pure guard: may this path be removed as an empty, never-initialised staging directory?
  .DESCRIPTION
    Applies the path, root, and leaf checks of Test-NetworkTuningRestoreStagingPath but does
    not require the marker file. The caller must still delete non-recursively and only an
    empty, non-reparse directory. Performs no filesystem access.
  #>
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [AllowNull()][AllowEmptyString()][string]$Path,
    [AllowNull()][AllowEmptyString()][string]$AllowedRoot,
    [Parameter(Mandatory)][bool]$WindowsRuntime
  )

  return Test-NetworkTuningRestoreStagingPath -Path $Path -AllowedRoot $AllowedRoot -MarkerPresent $true -WindowsRuntime $WindowsRuntime
}

function Remove-NetworkTuningEmptyRestoreStagingDirectory {
  [CmdletBinding()]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][bool]$WindowsRuntime
  )

  $guard = Test-NetworkTuningRestoreStagingEmptyRemoval `
    -Path $Path `
    -AllowedRoot (Get-NetworkTuningRestoreStagingAllowedRoot -WindowsRuntime $WindowsRuntime) `
    -WindowsRuntime $WindowsRuntime
  if (-not $guard.IsAllowed) {
    Write-Warning -Message ("Refusing to delete restore staging directory: {0}" -f $guard.Message)
    return
  }
  try {
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    if ($null -eq $item -or -not $item.PSIsContainer -or ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
      return
    }
    if (@([System.IO.Directory]::EnumerateFileSystemEntries($Path)).Count -gt 0) { return }
    [System.IO.Directory]::Delete($Path, $false)
  } catch {
    Write-Warning -Message ("Could not remove empty restore staging directory '{0}': {1}" -f $Path, $_.Exception.Message)
  }
}

function Remove-NetworkTuningRestoreStagingDirectory {
  [CmdletBinding()]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][bool]$WindowsRuntime,
    [string]$ExpectedNonce = ''
  )

  $markerPath = Join-Path -Path $Path -ChildPath $script:NetworkTuningRestoreStagingMarkerName
  $guard = Test-NetworkTuningRestoreStagingPath `
    -Path $Path `
    -AllowedRoot (Get-NetworkTuningRestoreStagingAllowedRoot -WindowsRuntime $WindowsRuntime) `
    -MarkerPresent (Test-Path -LiteralPath $markerPath -PathType Leaf) `
    -WindowsRuntime $WindowsRuntime
  if (-not $guard.IsAllowed) {
    Write-Warning -Message ("Refusing to delete restore staging directory: {0}" -f $guard.Message)
    return
  }
  # The staging directory and its marker must be real entries, never links or junctions.
  foreach ($entry in @($Path, $markerPath)) {
    $item = Get-Item -LiteralPath $entry -Force -ErrorAction SilentlyContinue
    if ($null -eq $item -or ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
      Write-Warning -Message ("Refusing to delete restore staging directory: {0} is missing or a reparse point" -f $entry)
      return
    }
  }
  if (-not [string]::IsNullOrEmpty($ExpectedNonce)) {
    $marker = [System.IO.File]::ReadAllText($markerPath)
    if ($marker -cne $ExpectedNonce) {
      Write-Warning -Message ("Refusing to delete restore staging directory: marker does not belong to this session: {0}" -f $Path)
      return
    }
  }

  try {
    Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction Stop
  } catch {
    Write-Warning -Message ("Could not remove restore staging directory '{0}': {1}" -f $Path, $_.Exception.Message)
  }
}

function Close-NetworkTuningRestoreStagingSession {
  [CmdletBinding()]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]$Session
  )

  if ($null -ne $Session.SentinelStream) {
    $Session.SentinelStream.Dispose()
  }
  if (-not [string]::IsNullOrWhiteSpace([string]$Session.Path)) {
    Remove-NetworkTuningRestoreStagingDirectory -Path ([string]$Session.Path) -WindowsRuntime ([bool]$Session.IsWindows) -ExpectedNonce ([string]$Session.Nonce)
  }
}

function Test-NetworkTuningRestoreStagingInvariant {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]$Session,
    [Parameter(Mandatory)][hashtable]$Manifest
  )

  try {
    $fullPath = [System.IO.Path]::GetFullPath([string]$Session.Path).TrimEnd([System.IO.Path]::DirectorySeparatorChar)
    $fullParent = [System.IO.Path]::GetFullPath([string]$Session.ParentPath).TrimEnd([System.IO.Path]::DirectorySeparatorChar)
    $actualParent = [System.IO.Path]::GetDirectoryName($fullPath).TrimEnd([System.IO.Path]::DirectorySeparatorChar)
    $comparison = if ([bool]$Session.IsWindows) { [System.StringComparison]::OrdinalIgnoreCase } else { [System.StringComparison]::Ordinal }
    if (-not $actualParent.Equals($fullParent, $comparison)) {
      return [pscustomobject]@{ IsValid = $false; Message = 'Restore staging session moved outside its trusted parent.' }
    }

    $expectedSentinelPath = Join-Path -Path $fullPath -ChildPath $script:NetworkTuningRestoreStagingMarkerName
    if (-not ([string]$Session.SentinelPath).Equals($expectedSentinelPath, $comparison)) {
      return [pscustomobject]@{ IsValid = $false; Message = 'Restore staging sentinel path changed.' }
    }

    $pathsToInspect = @($fullPath, $expectedSentinelPath)
    foreach ($artifactName in (Get-NetworkTuningExpectedBackupArtifactNames -Manifest $Manifest)) {
      $pathsToInspect += Join-Path -Path $fullPath -ChildPath $artifactName
    }
    foreach ($path in $pathsToInspect) {
      $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
      if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        return [pscustomobject]@{ IsValid = $false; Message = "Restore staging path became a reparse point: $path" }
      }
      if ([bool]$Session.IsWindows) {
        $pathCheck = Test-NetworkTuningWindowsAdminOnlyPath -Path $path
        if (-not $pathCheck.IsTrusted) {
          return [pscustomobject]@{ IsValid = $false; Message = $pathCheck.Message }
        }
      }
    }

    if ($null -eq $Session.SentinelStream -or $Session.SentinelStream.SafeFileHandle.IsClosed) {
      return [pscustomobject]@{ IsValid = $false; Message = 'Restore staging sentinel handle is not open.' }
    }
    # The session holds the sentinel open for writing, so on Windows a second
    # handle must share Write. Other writers stay blocked by the session
    # handle, which shares Read only.
    $sentinelBytes = [byte[]]::new(129)
    $sentinelLength = 0
    $sentinelReader = [System.IO.File]::Open($expectedSentinelPath, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
    try {
      while ($sentinelLength -lt $sentinelBytes.Length) {
        $bytesRead = $sentinelReader.Read($sentinelBytes, $sentinelLength, $sentinelBytes.Length - $sentinelLength)
        if ($bytesRead -eq 0) { break }
        $sentinelLength += $bytesRead
      }
    } finally {
      $sentinelReader.Dispose()
    }
    if ($sentinelLength -gt 128) {
      return [pscustomobject]@{ IsValid = $false; Message = 'Restore staging sentinel exceeds its maximum size.' }
    }
    $sentinelText = [System.Text.Encoding]::UTF8.GetString($sentinelBytes, 0, $sentinelLength)
    if ($sentinelText -cne [string]$Session.Nonce) {
      return [pscustomobject]@{ IsValid = $false; Message = 'Restore staging sentinel does not match the verified session.' }
    }

    $digestCheck = Test-NetworkTuningBackupArtifactDigests -BackupFolder $fullPath -Manifest $Manifest
    if (-not $digestCheck.IsValid) {
      return [pscustomobject]@{ IsValid = $false; Message = "Staged backup verification failed: $($digestCheck.Message)" }
    }
    $authorizationCheck = Test-NetworkTuningRestoreArtifactAuthorization -BackupFolder $fullPath -Manifest $Manifest
    if (-not $authorizationCheck.IsAuthorized) {
      return [pscustomobject]@{ IsValid = $false; Message = "Staged backup authorization failed: $($authorizationCheck.Message)" }
    }
  } catch {
    return [pscustomobject]@{ IsValid = $false; Message = "Restore staging invariant failed: $($_.Exception.Message)" }
  }

  return [pscustomobject]@{ IsValid = $true; Message = '' }
}

function Assert-NetworkTuningRestoreStagingInvariant {
  [CmdletBinding()]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]$Session,
    [Parameter(Mandatory)][hashtable]$Manifest
  )

  $check = Test-NetworkTuningRestoreStagingInvariant -Session $Session -Manifest $Manifest
  if (-not $check.IsValid) {
    $exception = [System.InvalidOperationException]::new($check.Message)
    $exception.Data['NetworkLantern.RestoreStagingInvariant'] = $true
    throw $exception
  }
}

function Assert-NetworkTuningRestoreStagingConsumerInvariant {
  [CmdletBinding()]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]$Session,
    [Parameter(Mandatory)][hashtable]$Manifest
  )

  Assert-NetworkTuningRestoreStagingInvariant -Session $Session -Manifest $Manifest
}

function Copy-NetworkTuningBoundedArtifactToStaging {
  [CmdletBinding()]
  [OutputType([string])]
  param(
    [Parameter(Mandatory)][string]$SourcePath,
    [Parameter(Mandatory)][string]$DestinationPath,
    [Parameter(Mandatory)][long]$MaximumBytes,
    [Parameter(Mandatory)][string]$ExpectedSha256,
    [Parameter(Mandatory)][bool]$WindowsRuntime
  )

  $sourceCheck = Test-NetworkTuningBackupPathEntryIsRegular -Path $SourcePath
  if (-not $sourceCheck.IsTrusted) { throw $sourceCheck.Message }
  $destinationStream = $null
  $hasher = $null
  try {
    # The source is opened once, with sharing that denies replacement and
    # writes. Hashing and copying consume the same bounded byte stream.
    $destinationStream = [System.IO.File]::Open($DestinationPath, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    $hasher = [System.Security.Cryptography.IncrementalHash]::CreateHash([System.Security.Cryptography.HashAlgorithmName]::SHA256)
    Invoke-NetworkTuningBoundedFileStream -Path $SourcePath -MaximumBytes $MaximumBytes -OnChunk {
      param($buffer, $read)
      $hasher.AppendData($buffer, 0, $read)
      $destinationStream.Write($buffer, 0, $read)
    } | Out-Null
    $destinationStream.Flush($true)
    $hash = [System.Convert]::ToHexString($hasher.GetHashAndReset())
  } finally {
    if ($null -ne $hasher) { $hasher.Dispose() }
    if ($null -ne $destinationStream) { $destinationStream.Dispose() }
  }

  if ($hash -cne $ExpectedSha256) { throw "Backup artifact digest mismatch while staging: $([System.IO.Path]::GetFileName($SourcePath))" }
  $destinationCheck = Test-NetworkTuningBackupPathEntryIsRegular -Path $DestinationPath
  if (-not $destinationCheck.IsTrusted) { throw $destinationCheck.Message }
  if ($WindowsRuntime) {
    Protect-NetworkTuningAdminOnlyFile -Path $DestinationPath -Confirm:$false
    $destinationAclCheck = Test-NetworkTuningWindowsAdminOnlyPath -Path $DestinationPath
    if (-not $destinationAclCheck.IsTrusted) { throw $destinationAclCheck.Message }
  }
  return $hash
}

function Copy-NetworkTuningVerifiedBackupToStaging {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)][string]$BackupFolder,
    [Parameter(Mandatory)][hashtable]$Manifest
  )

  $session = New-NetworkTuningRestoreStagingSession -Confirm:$false
  try {
    foreach ($artifactName in (Get-NetworkTuningExpectedBackupArtifactNames -Manifest $Manifest)) {
      $sourcePath = Join-Path -Path $BackupFolder -ChildPath $artifactName
      $destinationPath = Join-Path -Path $session.Path -ChildPath $artifactName
      $expectedDigest = [string]$Manifest.ArtifactDigests[$artifactName]
      Copy-NetworkTuningBoundedArtifactToStaging -SourcePath $sourcePath -DestinationPath $destinationPath -MaximumBytes (Get-NetworkTuningBackupArtifactMaximumBytes -FileName $artifactName) -ExpectedSha256 $expectedDigest -WindowsRuntime ([bool]$session.IsWindows) | Out-Null
    }

    Assert-NetworkTuningRestoreStagingInvariant -Session $session -Manifest $Manifest
    return $session
  } catch {
    Close-NetworkTuningRestoreStagingSession -Session $session
    throw
  }
}
