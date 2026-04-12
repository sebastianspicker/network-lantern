function Get-UjRegistryPathForRegExe {
  [CmdletBinding()]
  [OutputType([string])]
  param([Parameter(Mandatory)][string]$Path)
  # Converts 'HKLM:\...' to 'HKLM\...' for reg.exe compatibility
  return $Path -replace '^HKLM:', 'HKLM' -replace '^HKCU:', 'HKCU' -replace '^HKCR:', 'HKCR' -replace '^HKU:', 'HKU'
}

function Export-UjRegistryKey {
  [CmdletBinding()]
  [OutputType([bool])]
  param(
    [Parameter(Mandatory)]
    [string]$RegistryPath,

    [Parameter(Mandatory)]
    [string]$OutFile
  )

  try {
    $outDir = Split-Path -Path $OutFile -Parent
    if ($outDir -and -not (Test-Path -Path $outDir)) {
      New-Item -ItemType Directory -Path $outDir -Force | Out-Null
    }

    $normalizedPath = Get-UjRegistryPathForRegExe -Path $RegistryPath
    $result = & reg.exe export $normalizedPath $OutFile /y 2>&1
    if ($LASTEXITCODE -ne 0) {
      Write-Warning -Message ("Could not back up registry key '{0}'. The key may not exist yet (this is normal on a fresh system). Error code: {1}" -f $normalizedPath, $LASTEXITCODE)
      return $false
    }
    return $true
  } catch {
    Write-Warning -Message ("Could not back up registry key '{0}': {1}" -f $RegistryPath, $_.Exception.Message)
    return $false
  }
}

function Import-UjRegistryFile {
  [CmdletBinding()]
  [OutputType([bool])]
  param(
    [Parameter(Mandatory)]
    [string]$InFile
  )

  if (-not (Test-Path -Path $InFile)) {
    Write-Warning -Message ("Could not restore registry settings: backup file not found at '{0}'. If you haven't run a backup yet, run Backup first." -f $InFile)
    return $false
  }

  try {
    $null = & reg.exe import $InFile 2>&1
    if ($LASTEXITCODE -ne 0) {
      Write-Warning -Message ("Could not restore registry settings from '{0}'. The backup file may be corrupted or from a different PC (error code {1})." -f $InFile, $LASTEXITCODE)
      return $false
    }
    return $true
  } catch {
    Write-Warning -Message ("Could not restore registry settings from '{0}': {1}" -f $InFile, $_.Exception.Message)
    return $false
  }
}

function Set-UjRegistryValue {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]
    [string]$Key,

    [Parameter(Mandatory)]
    [string]$Name,

    [Parameter(Mandatory)]
    [ValidateSet('DWord', 'String')]
    [string]$Type,

    [Parameter(Mandatory)]
    [AllowNull()]
    $Value
  )

  if (-not (Test-Path -Path $Key)) {
    if ($PSCmdlet.ShouldProcess($Key, 'Create registry key')) {
      New-Item -Path $Key -Force | Out-Null
    }
  }

  if (-not $PSCmdlet.ShouldProcess((Join-Path -Path $Key -ChildPath $Name), ("Set registry value ({0})" -f $Type))) {
    return
  }

  if ($Type -eq 'DWord') {
    New-ItemProperty -Path $Key -Name $Name -PropertyType DWord -Value ([int]$Value) -Force | Out-Null
    return
  }

  New-ItemProperty -Path $Key -Name $Name -PropertyType String -Value ([string]$Value) -Force | Out-Null
}
