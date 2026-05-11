<#
.SYNOPSIS
Lightweight secret scan over the repository (no output of matches).

.DESCRIPTION
Scans files for common secret patterns. Excludes .git directory (path-agnostic).
Use in CI and locally; exits with 1 if any pattern is found.
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
$resolvedPath = [System.IO.Path]::GetFullPath($Path)
$gitDir = Join-Path $resolvedPath '.git'

if ((Test-Path -LiteralPath $gitDir) -and (Get-Command -Name git -ErrorAction SilentlyContinue)) {
  $trackedPaths = & git -C $resolvedPath ls-files -z
  $files = @($trackedPaths -split "`0" | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | ForEach-Object {
      $candidate = Join-Path $resolvedPath $_
      if (Test-Path -LiteralPath $candidate -PathType Leaf) {
        Get-Item -LiteralPath $candidate
      }
    })
} else {
  $files = Get-ChildItem -LiteralPath $resolvedPath -Recurse -File -Force -ErrorAction SilentlyContinue |
    Where-Object {
      $full = $_.FullName
      $parts = $full.Split([IO.Path]::DirectorySeparatorChar)
      $parts -notcontains '.git' -and
        $parts -notcontains '.cache' -and
        $parts -notcontains 'artifacts'
    }
}

$files = @($files | Where-Object {
    $_.Name -ne $selfName -and
      $_.Extension -notin $binaryExts
  })

foreach ($pattern in $patterns) {
  $hits = $files | Select-String -Pattern $pattern
  if ($hits) {
    $hitCount += $hits.Count
  }
}

if ($hitCount -gt 0) {
  Write-Error "Potential secrets detected ($hitCount). Review locally; output suppressed."
  exit 1
}
Write-Verbose "Secret scan: no matches."
