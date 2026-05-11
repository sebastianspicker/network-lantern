[CmdletBinding()]
param(
  [string]$Filter
)

$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot 'Get-RepoRoot.ps1')
$repoRoot = Get-RepoRoot
$testsDir = Join-Path $repoRoot 'tests'

if (-not (Get-Module -ListAvailable -Name Pester | Where-Object { $_.Version -ge [version]'5.0.0' })) {
  Write-Error "Pester 5+ is required. Install with: Install-Module Pester -MinimumVersion 5.7.1 -Scope CurrentUser -Force"
  exit 1
}

Import-Module -Name Pester -MinimumVersion 5.0.0 -ErrorAction Stop

$config = [PesterConfiguration]::Default
$config.Run.Path = $testsDir
$config.Run.Exit = $true
$config.Run.PassThru = $true
$config.Output.Verbosity = 'Detailed'
$config.TestResult.Enabled = $true
$config.TestResult.OutputFormat = 'NUnitXml'
$config.TestResult.OutputPath = Join-Path $repoRoot 'artifacts/testResults.xml'
$config.Should.ErrorAction = 'Stop'

if ($Filter) {
  $config.Filter.FullName = "*$Filter*"
}

$result = Invoke-Pester -Configuration $config

if ($result.FailedCount -gt 0) {
  exit 1
}
