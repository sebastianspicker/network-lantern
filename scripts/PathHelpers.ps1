<#
.SYNOPSIS
Shared path helper utilities for CLI/GUI scripts.
#>

# INTENTIONAL DUPLICATION: Test-PathUnderBase is duplicated from src/Private/Common.ps1.
# This copy exists for CLI/GUI scripts that dot-source this file before the module loads.
# The module's private copy is the canonical source. If you change the logic, you MUST
# update both copies to keep them in sync.
function Test-PathUnderBase {
  [CmdletBinding()]
  [OutputType([bool])]
  param(
    [Parameter(Mandatory)]
    [string]$BasePath,
    [Parameter(Mandatory)]
    [string]$CandidatePath
  )
  $baseFull = [System.IO.Path]::GetFullPath($BasePath)
  $candidateFull = [System.IO.Path]::GetFullPath($CandidatePath)
  $separators = @([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
  $baseWithSeparator = $baseFull.TrimEnd($separators) + [System.IO.Path]::DirectorySeparatorChar
  $comparison = if ($IsWindows) { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }
  return $candidateFull.Equals($baseFull, $comparison) -or $candidateFull.StartsWith($baseWithSeparator, $comparison)
}

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

function Open-FolderOrFile {
  [CmdletBinding()]
  param(
    [Parameter(Mandatory)]
    [string]$Path
  )
  $openerPath = [System.IO.Path]::GetFullPath($Path)
  if ($IsWindows) {
    Start-Process explorer.exe -ArgumentList @($openerPath)
  }
  elseif ($IsMacOS) {
    Start-Process 'open' -ArgumentList @($openerPath)
  }
  else {
    $xdgOpen = Get-Command 'xdg-open' -ErrorAction SilentlyContinue
    if ($xdgOpen) {
      Start-Process 'xdg-open' -ArgumentList @($openerPath)
    }
    else {
      Write-Warning "No file opener found. Path: $openerPath"
    }
  }
}
