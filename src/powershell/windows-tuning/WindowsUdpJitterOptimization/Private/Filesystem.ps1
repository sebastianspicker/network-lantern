function New-UjDirectory {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]
    [string]$Path
  )

  if ([string]::IsNullOrWhiteSpace($Path)) {
    throw 'Path must not be null or empty.'
  }

  if (Test-Path -Path $Path) {
    return
  }

  if ($PSCmdlet.ShouldProcess($Path, 'Create directory')) {
    New-Item -ItemType Directory -Path $Path -Force | Out-Null
  }
}

function Test-UjUnsafeBackupFolder {
  [CmdletBinding()]
  [OutputType([bool])]
  param(
    [Parameter(Mandatory)]
    [string]$Path
  )

  if ([string]::IsNullOrWhiteSpace($Path)) {
    return $false
  }

  $canonicalRaw = $Path.Replace('/', '\').TrimEnd('\')

  $canonicalFull = $canonicalRaw
  try {
    $canonicalFull = [System.IO.Path]::GetFullPath($Path).Replace('/', '\').TrimEnd('\')
  } catch {
    # Keep original canonical input when full path resolution is not possible.
    Write-Verbose -Message ("Path resolution failed for '{0}': {1}" -f $Path, $_.Exception.Message)
  }

  $windirSystem32 = $null
  if (-not [string]::IsNullOrWhiteSpace($env:windir)) {
    $windirSystem32 = Join-Path -Path $env:windir -ChildPath 'System32'
  }

  $sensitiveRoots = [System.Collections.Generic.List[string]]::new()
  foreach ($root in @(
      $env:windir,
      $windirSystem32,
      $env:ProgramFiles,
      ${env:ProgramFiles(x86)},
      'C:\Windows',
      'C:\Windows\System32',
      'C:\Program Files',
      'C:\Program Files (x86)'
    )) {
    if (-not [string]::IsNullOrWhiteSpace($root)) {
      $sensitiveRoots.Add($root.Replace('/', '\').TrimEnd('\')) | Out-Null
    }
  }

  $pathCandidates = [System.Collections.Generic.List[string]]::new()
  if (-not [string]::IsNullOrWhiteSpace($canonicalFull)) { $pathCandidates.Add($canonicalFull) | Out-Null }
  if (-not [string]::IsNullOrWhiteSpace($canonicalRaw)) { $pathCandidates.Add($canonicalRaw) | Out-Null }

  foreach ($candidate in $pathCandidates) {
    foreach ($root in $sensitiveRoots) {
      if ($candidate.Equals($root, [System.StringComparison]::OrdinalIgnoreCase) -or
          $candidate.StartsWith(($root + '\'), [System.StringComparison]::OrdinalIgnoreCase)) {
        return $true
      }
    }
  }

  if ($canonicalRaw -match '(^|\\)windows(\\|$)' -or
      $canonicalRaw -match '(^|\\)windows\\system32(\\|$)' -or
      $canonicalRaw -match '(^|\\)program files( \(x86\))?(\\|$)') {
    return $true
  }

  return $false
}
