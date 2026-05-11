[CmdletBinding()]
param(
  [switch]$NoInstall
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot

Write-Information "PowerShell version: $($PSVersionTable.PSVersion)" -InformationAction Continue
Write-Information "PSModulePath: $env:PSModulePath" -InformationAction Continue

# Module version pins — keep in sync with the cache-key comment in .github/workflows/ci.yml.
# Update both files when bumping either version.
$requiredModules = @{
  PSScriptAnalyzer = '1.24.0'
  Pester           = '5.7.1'
}

function Test-ExactModuleInstalled {
  param(
    [string]$Name,
    [string]$Version
  )

  return [bool](Get-Module -ListAvailable -Name $Name | Where-Object { $_.Version -eq [version]$Version } | Select-Object -First 1)
}

$missingModules = @(
  $requiredModules.GetEnumerator() | Where-Object { -not (Test-ExactModuleInstalled -Name $_.Key -Version $_.Value) }
)

if ($NoInstall -and $missingModules.Count -gt 0) {
  [Console]::Error.WriteLine("Missing required PowerShell module(s): $($missingModules.Key -join ', '). Re-run without -NoInstall to install them, or install them manually.")
  exit 1
}

if ($missingModules.Count -eq 0) {
  Write-Information 'Required PowerShell modules already available; skipping installation.' -InformationAction Continue
} elseif (Get-Command -Name Install-PSResource -ErrorAction SilentlyContinue) {
  $repo = Get-PSResourceRepository -Name PSGallery -ErrorAction SilentlyContinue
  if (-not $repo) {
    Register-PSResourceRepository -PSGallery
    $repo = Get-PSResourceRepository -Name PSGallery -ErrorAction Stop
  }

  $restoreTrust = $false
  if (-not $repo.Trusted) {
    Set-PSResourceRepository -Name PSGallery -Trusted
    $restoreTrust = $true
  }

  try {
    foreach ($entry in $missingModules) {
      $name = $entry.Key
      $version = $entry.Value
      Write-Information "Installing $name $version via PSResourceGet..." -InformationAction Continue
      Install-PSResource -Name $name -Version $version -Scope CurrentUser -Repository PSGallery -TrustRepository -ErrorAction Stop
    }
  } finally {
    if ($restoreTrust) {
      Set-PSResourceRepository -Name PSGallery -Trusted:$false
    }
  }
} else {
  $repo = Get-PSRepository -Name PSGallery -ErrorAction SilentlyContinue
  if (-not $repo) {
    Register-PSRepository -Default
    $repo = Get-PSRepository -Name PSGallery -ErrorAction Stop
  }

  $restorePolicy = $false
  if ($repo.InstallationPolicy -ne 'Trusted') {
    Set-PSRepository -Name PSGallery -InstallationPolicy Trusted
    $restorePolicy = $true
  }

  try {
    try {
      Install-PackageProvider -Name NuGet -Force -Scope CurrentUser -ErrorAction Stop | Out-Null
    } catch {
      Write-Warning "Install-PackageProvider failed: $($_.Exception.Message)"
    }

    foreach ($entry in $missingModules) {
      $name = $entry.Key
      $version = [version]$entry.Value
      Write-Information "Installing $name $version via PowerShellGet..." -InformationAction Continue
      Install-Module -Name $name -RequiredVersion $version -Scope CurrentUser -Repository PSGallery -Force -ErrorAction Stop
    }
  } finally {
    if ($restorePolicy) {
      Set-PSRepository -Name PSGallery -InstallationPolicy $repo.InstallationPolicy
    }
  }
}

$pathsToAnalyze = @(
  (Join-Path $repoRoot 'apps')
  (Join-Path $repoRoot 'src')
  (Join-Path $repoRoot 'scripts')
  (Join-Path $repoRoot 'tests')
  (Join-Path $repoRoot 'Invoke-NetworkDiagnostics.ps1')
)
$settingsPath = Join-Path $repoRoot 'PSScriptAnalyzerSettings.psd1'
$scriptAnalyzerResults = foreach ($path in $pathsToAnalyze) {
  Invoke-ScriptAnalyzer -Path $path -Recurse -Settings $settingsPath
}
if ($scriptAnalyzerResults) {
  $scriptAnalyzerResults | Sort-Object ScriptName, Line | Format-Table -AutoSize | Out-String | Write-Output
  throw "PSScriptAnalyzer found $(@($scriptAnalyzerResults).Count) issue(s)."
}

$artifactsDir = Join-Path -Path $repoRoot -ChildPath 'artifacts'
if (-not (Test-Path -LiteralPath $artifactsDir)) {
  New-Item -Path $artifactsDir -ItemType Directory -Force | Out-Null
}

$pesterConfiguration = [PesterConfiguration]::Default
$pesterConfiguration.Run.Path = Join-Path $repoRoot 'tests'
$pesterConfiguration.Run.Exit = $false
$pesterConfiguration.Run.PassThru = $true
$pesterConfiguration.Output.Verbosity = 'Detailed'
$pesterConfiguration.TestResult.Enabled = $true
$pesterConfiguration.TestResult.OutputFormat = 'NUnitXml'
$pesterConfiguration.TestResult.OutputPath = Join-Path $artifactsDir 'testResults.xml'
$pesterConfiguration.Should.ErrorAction = 'Stop'

$pesterResult = Invoke-Pester -Configuration $pesterConfiguration
if (-not $pesterResult -or $pesterResult.FailedCount -gt 0) {
  throw "Pester reported $($pesterResult.FailedCount) failed test(s)."
}
