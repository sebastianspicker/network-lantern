function Test-UjIsAdministrator {
  [CmdletBinding()]
  [OutputType([bool])]
  param()

  try {
    $principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
  } catch {
    Write-Verbose -Message 'Admin check failed (non-Windows or restricted platform).'
    return $false
  }
}

function Assert-UjAdministrator {
  [CmdletBinding()]
  [OutputType([void])]
  param()

  if (-not (Test-UjIsAdministrator)) {
    throw 'Please run as Administrator.'
  }
}

function Get-UjGuidFromText {
  [CmdletBinding()]
  [OutputType([string])]
  param(
    [Parameter(Mandatory)]
    [string]$Text
  )

  if ($Text -match '\{([0-9a-fA-F-]+)\}') {
    return $Matches[0]
  }

  if ($Text -match '([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})') {
    return '{' + $Matches[1] + '}'
  }

  return $null
}
