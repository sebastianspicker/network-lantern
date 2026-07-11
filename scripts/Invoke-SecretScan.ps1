<#
.SYNOPSIS
Lightweight secret scan over the repository (no output of matches).

.DESCRIPTION
Scans tracked files and untracked files that are not ignored by Git for common
secret patterns. Use in CI and locally; exits with 1 if any pattern is found.
#>
[CmdletBinding()]
param(
  [string]$Path = (Join-Path (Split-Path -Parent $PSScriptRoot) '.')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$patterns = @(
  '-----BEGIN (RSA|DSA|EC|OPENSSH|PGP|ENCRYPTED) PRIVATE KEY-----',
  'AKIA[0-9A-Z]{16}',
  'ASIA[0-9A-Z]{16}',
  'ghp_[A-Za-z0-9]{36}',
  'gho_[A-Za-z0-9]{36}',
  'ghu_[A-Za-z0-9]{36}',
  'ghs_[A-Za-z0-9]{36}',
  'ghr_[A-Za-z0-9]{36}',
  'github_pat_[A-Za-z0-9_]{22,}',
  'xox[baprs]-[A-Za-z0-9-]{10,48}',
  'sk-[A-Za-z0-9]{20,}',
  'AIza[A-Za-z0-9_\\-]{35}',
  'eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}',
  'npm_[A-Za-z0-9]{36}',
  'pypi-[A-Za-z0-9]{60,}',
  'SG\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9_-]{43}',
  'sk_live_[A-Za-z0-9]{24,}',
  'rk_live_[A-Za-z0-9]{24,}'
)

$hitCount = 0
$selfName = [System.IO.Path]::GetFileName($MyInvocation.MyCommand.Path)
$binaryExts = @('.exe','.dll','.zip','.gz','.tar','.png','.jpg','.gif','.ico','.woff','.woff2','.ttf','.eot','.pdf')
$root = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).Path
$git = Get-Command -Name git -ErrorAction SilentlyContinue

if ((Test-Path -LiteralPath (Join-Path $root '.git')) -and $git) {
  $relativeFiles = @(& $git.Source -C $root ls-files --cached --others --exclude-standard)
  if ($LASTEXITCODE -ne 0) {
    throw 'Could not enumerate Git publication candidates for secret scanning.'
  }
  $files = @(
    foreach ($relativeFile in $relativeFiles) {
      $fullPath = Join-Path -Path $root -ChildPath $relativeFile
      if (-not (Test-Path -LiteralPath $fullPath -PathType Leaf)) { continue }
      Get-Item -LiteralPath $fullPath -Force -ErrorAction Stop
    }
  )
} else {
  $files = @(Get-ChildItem -LiteralPath $root -Recurse -File -Force -ErrorAction SilentlyContinue |
      Where-Object {
        $parts = $_.FullName.Split([IO.Path]::DirectorySeparatorChar)
        $parts -notcontains '.git' -and
          $parts -notcontains '.cache' -and
          $parts -notcontains 'artifacts'
      })
}

$files = @(
  $files | Where-Object {
    $_.Name -ne $selfName -and $_.Extension -notin $binaryExts
  }
)

foreach ($pattern in $patterns) {
  $hits = if ($files.Count -gt 0) {
    $files | Select-String -Pattern $pattern
  }
  if ($hits) {
    $hitCount += $hits.Count
  }
}

if ($hitCount -gt 0) {
  Write-Error "Potential secrets detected ($hitCount). Review locally; output suppressed."
  exit 1
}
Write-Verbose "Secret scan: no matches."
