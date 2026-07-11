[CmdletBinding()]
param(
  [string]$Filter
)

$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot 'Get-RepoRoot.ps1')
$repoRoot = Get-RepoRoot
$testsDir = Join-Path $repoRoot 'tests'

if (-not $Filter) {
  & (Join-Path $repoRoot 'scripts/ci.ps1')
  exit $LASTEXITCODE
}

Write-Warning 'Invoke-Tests.ps1 -Filter runs a filtered Pester subset only. Use scripts/ci.ps1 or ./scripts/ci-local.sh for the full verification gate.'

if (-not (Get-Module -ListAvailable -Name Pester | Where-Object { $_.Version -ge [version]'5.0.0' })) {
  Write-Error "Pester 5+ is required. Install with: Install-Module Pester -MinimumVersion 5.7.1 -Scope CurrentUser -Force"
  exit 1
}
Import-Module Pester -MinimumVersion 5.0.0 -ErrorAction Stop

$testResultsDir = Join-Path $repoRoot 'artifacts'
New-Item -ItemType Directory -Path $testResultsDir -Force | Out-Null

$config = [PesterConfiguration]::Default
$config.Run.Path = $testsDir
$config.Run.Exit = $true
$config.Run.PassThru = $true
$config.Output.Verbosity = 'Detailed'
$config.TestResult.Enabled = $true
$config.TestResult.OutputFormat = 'NUnitXml'
$config.TestResult.OutputPath = Join-Path $testResultsDir 'testResults.xml'
$config.Should.ErrorAction = 'Stop'

$config.Filter.FullName = "*$Filter*"

$result = Invoke-Pester -Configuration $config

if ($result.FailedCount -gt 0) {
  exit 1
}
