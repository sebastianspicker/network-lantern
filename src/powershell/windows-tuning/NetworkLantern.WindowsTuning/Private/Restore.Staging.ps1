function New-UjRestoreStagingSession {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([pscustomobject])]
  param()

  $isWindowsRuntime = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)
  $parentPath = if ($isWindowsRuntime) { Get-UjWindowsRestoreStagingRoot } else { [System.IO.Path]::GetTempPath() }
  $stagingPath = Join-Path -Path $parentPath -ChildPath ("NetworkLantern-Restore-{0}" -f [guid]::NewGuid().ToString('N'))
  if (-not $PSCmdlet.ShouldProcess($stagingPath, 'Create restricted restore staging session')) {
    throw 'Restore staging session creation was declined.'
  }

  $sentinelStream = $null
  try {
    if ($isWindowsRuntime) {
      $stagingPath = Initialize-UjAdminOnlyDirectory -Path $stagingPath -Confirm:$false
    } else {
      [System.IO.Directory]::CreateDirectory($stagingPath) | Out-Null
      $stagingItem = Get-Item -LiteralPath $stagingPath -Force -ErrorAction Stop
      if (($stagingItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw 'Restore staging session unexpectedly resolved to a reparse point.'
      }
    }

    $nonce = [guid]::NewGuid().ToString('N')
    $sentinelPath = Join-Path -Path $stagingPath -ChildPath '.restore-session'
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
      Protect-UjAdminOnlyFile -Path $sentinelPath -Confirm:$false
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
    Remove-Item -LiteralPath $stagingPath -Recurse -Force -ErrorAction SilentlyContinue
    throw "Could not create a trusted restore staging session: $($_.Exception.Message)"
  }
}

function Close-UjRestoreStagingSession {
  [CmdletBinding()]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]$Session
  )

  if ($null -ne $Session.SentinelStream) {
    $Session.SentinelStream.Dispose()
  }
  if (-not [string]::IsNullOrWhiteSpace([string]$Session.Path)) {
    Remove-Item -LiteralPath $Session.Path -Recurse -Force -ErrorAction SilentlyContinue
  }
}

function Test-UjRestoreStagingInvariant {
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

    $expectedSentinelPath = Join-Path -Path $fullPath -ChildPath '.restore-session'
    if (-not ([string]$Session.SentinelPath).Equals($expectedSentinelPath, $comparison)) {
      return [pscustomobject]@{ IsValid = $false; Message = 'Restore staging sentinel path changed.' }
    }

    $pathsToInspect = @($fullPath, $expectedSentinelPath)
    foreach ($artifactName in (Get-UjExpectedBackupArtifactNames -Manifest $Manifest)) {
      $pathsToInspect += Join-Path -Path $fullPath -ChildPath $artifactName
    }
    foreach ($path in $pathsToInspect) {
      $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
      if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        return [pscustomobject]@{ IsValid = $false; Message = "Restore staging path became a reparse point: $path" }
      }
      if ([bool]$Session.IsWindows) {
        $pathCheck = Test-UjWindowsAdminOnlyPath -Path $path
        if (-not $pathCheck.IsTrusted) {
          return [pscustomobject]@{ IsValid = $false; Message = $pathCheck.Message }
        }
      }
    }

    if ($null -eq $Session.SentinelStream -or $Session.SentinelStream.SafeFileHandle.IsClosed) {
      return [pscustomobject]@{ IsValid = $false; Message = 'Restore staging sentinel handle is not open.' }
    }
    $sentinelText = Get-Content -LiteralPath $expectedSentinelPath -Raw -ErrorAction Stop
    if ($sentinelText -cne [string]$Session.Nonce) {
      return [pscustomobject]@{ IsValid = $false; Message = 'Restore staging sentinel does not match the verified session.' }
    }

    $digestCheck = Test-UjBackupArtifactDigests -BackupFolder $fullPath -Manifest $Manifest
    if (-not $digestCheck.IsValid) {
      return [pscustomobject]@{ IsValid = $false; Message = "Staged backup verification failed: $($digestCheck.Message)" }
    }
  } catch {
    return [pscustomobject]@{ IsValid = $false; Message = "Restore staging invariant failed: $($_.Exception.Message)" }
  }

  return [pscustomobject]@{ IsValid = $true; Message = '' }
}

function Assert-UjRestoreStagingInvariant {
  [CmdletBinding()]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]$Session,
    [Parameter(Mandatory)][hashtable]$Manifest
  )

  $check = Test-UjRestoreStagingInvariant -Session $Session -Manifest $Manifest
  if (-not $check.IsValid) {
    $exception = [System.InvalidOperationException]::new($check.Message)
    $exception.Data['NetworkLantern.RestoreStagingInvariant'] = $true
    throw $exception
  }
}

function Assert-UjRestoreStagingConsumerInvariant {
  [CmdletBinding()]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]$Session,
    [Parameter(Mandatory)][hashtable]$Manifest
  )

  Assert-UjRestoreStagingInvariant -Session $Session -Manifest $Manifest
}

function Copy-UjVerifiedBackupToStaging {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)][string]$BackupFolder,
    [Parameter(Mandatory)][hashtable]$Manifest
  )

  $session = New-UjRestoreStagingSession -Confirm:$false
  try {
    foreach ($artifactName in (Get-UjExpectedBackupArtifactNames -Manifest $Manifest)) {
      $sourcePath = Join-Path -Path $BackupFolder -ChildPath $artifactName
      $destinationPath = Join-Path -Path $session.Path -ChildPath $artifactName
      Copy-Item -LiteralPath $sourcePath -Destination $destinationPath -Force -ErrorAction Stop
      if ([bool]$session.IsWindows) {
        Protect-UjAdminOnlyFile -Path $destinationPath -Confirm:$false
      }
    }

    Assert-UjRestoreStagingInvariant -Session $session -Manifest $Manifest
    return $session
  } catch {
    Close-UjRestoreStagingSession -Session $session
    throw
  }
}

