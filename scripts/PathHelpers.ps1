<#
.SYNOPSIS
Shared path helper utilities for CLI/GUI scripts.
#>

$pathUnderBaseHelper = Join-Path $PSScriptRoot '../src/powershell/throughput/Private/Test-PathUnderBase.ps1'
if (-not (Test-Path -LiteralPath $pathUnderBaseHelper -PathType Leaf)) {
  throw "Shared path helper is missing: $pathUnderBaseHelper"
}
. $pathUnderBaseHelper

function Resolve-ConfigPath {
  [CmdletBinding()]
  [OutputType([string])]
  param(
    [Parameter(Mandatory)]
    [string]$Path,
    [string]$BasePath = (Get-Location).Path,
    [switch]$RequireExistingFile
  )
  if ($Path -match '[\x00-\x1f]') {
    throw "Configuration path contains control characters."
  }
  $base = [System.IO.Path]::GetFullPath($BasePath)
  $resolved = if ([System.IO.Path]::IsPathRooted($Path)) {
    [System.IO.Path]::GetFullPath($Path)
  }
  else {
    $candidate = [System.IO.Path]::GetFullPath([System.IO.Path]::Combine($base, $Path))
    if (-not (Test-PathUnderBase -BasePath $base -CandidatePath $candidate)) {
      throw "Configuration path must be under the current directory. Resolved: $candidate"
    }
    $candidate
  }
  if ($RequireExistingFile -and -not (Test-Path -LiteralPath $resolved -PathType Leaf)) {
    throw "Configuration path is not a file: $resolved"
  }
  return $resolved
}

function Invoke-PathOpenerProcess {
  [CmdletBinding()]
  param(
    [Parameter(Mandatory)]
    [string]$Command,
    [Parameter(Mandatory)]
    [string]$Path
  )

  $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
  $startInfo.FileName = $Command
  $startInfo.UseShellExecute = $false
  $startInfo.ArgumentList.Add($Path)
  [System.Diagnostics.Process]::Start($startInfo) | Out-Null
}

function Open-FolderOrFile {
  [CmdletBinding()]
  param(
    [Parameter(Mandatory)]
    [string]$Path
  )
  $openerPath = [System.IO.Path]::GetFullPath($Path)
  $openerCommand = $null
  if ($IsWindows) {
    $openerCommand = 'explorer.exe'
  }
  elseif ($IsMacOS) {
    $openerCommand = 'open'
  }
  else {
    $xdgOpen = Get-Command 'xdg-open' -ErrorAction SilentlyContinue
    if ($xdgOpen) {
      $openerCommand = $xdgOpen.Source
    }
    else {
      Write-Warning "No file opener found. Path: $openerPath"
      return
    }
  }

  Invoke-PathOpenerProcess -Command $openerCommand -Path $openerPath
}
