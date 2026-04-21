<#
.SYNOPSIS
Returns the repository root directory (directory that contains the scripts/ folder).
.DESCRIPTION
When run from repo root, returns current directory. When run from scripts/, returns parent directory.
Dot-source this script and call Get-RepoRoot to get the path.
#>
function Get-RepoRoot {
  if ($PSScriptRoot) {
    return (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
  }
  return (Get-Location).Path
}
