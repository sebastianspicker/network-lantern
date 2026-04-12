Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot

Write-Information "PowerShell version: $($PSVersionTable.PSVersion)" -InformationAction Continue
Write-Information "PSModulePath: $env:PSModulePath" -InformationAction Continue

$requiredModules = @{
  PSScriptAnalyzer = '1.24.0'
  Pester           = '5.7.1'
}

if (Get-Command -Name Install-PSResource -ErrorAction SilentlyContinue) {
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
    foreach ($entry in $requiredModules.GetEnumerator()) {
      $name = $entry.Key
      $version = $entry.Value
      $installed = Get-Module -ListAvailable -Name $name | Where-Object { $_.Version -eq [version]$version } | Select-Object -First 1
      if ($installed) {
        Write-Information "Using $name $($installed.Version)" -InformationAction Continue
        continue
      }

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

    foreach ($entry in $requiredModules.GetEnumerator()) {
      $name = $entry.Key
      $version = [version]$entry.Value
      $installed = Get-Module -ListAvailable -Name $name | Where-Object { $_.Version -eq $version } | Select-Object -First 1
      if ($installed) {
        Write-Information "Using $name $($installed.Version)" -InformationAction Continue
        continue
      }

      Write-Information "Installing $name $version via PowerShellGet..." -InformationAction Continue
      Install-Module -Name $name -RequiredVersion $version -Scope CurrentUser -Repository PSGallery -Force -ErrorAction Stop
    }
  } finally {
    if ($restorePolicy) {
      Set-PSRepository -Name PSGallery -InstallationPolicy $repo.InstallationPolicy
    }
  }
}

$scriptAnalyzerResults = Invoke-ScriptAnalyzer -Path $repoRoot -Recurse
if ($scriptAnalyzerResults) {
  $scriptAnalyzerResults | Sort-Object ScriptName, Line | Format-Table -AutoSize | Out-String | Write-Output
  throw "PSScriptAnalyzer found $(@($scriptAnalyzerResults).Count) issue(s)."
}

$pesterResultsDir = Join-Path -Path $repoRoot -ChildPath 'test-results'
if (-not (Test-Path -LiteralPath $pesterResultsDir)) {
  New-Item -Path $pesterResultsDir -ItemType Directory -Force | Out-Null
}

Push-Location -Path $pesterResultsDir
try {
  Invoke-Pester -Path (Join-Path -Path $repoRoot -ChildPath 'tests') -CI
} finally {
  Pop-Location
}
