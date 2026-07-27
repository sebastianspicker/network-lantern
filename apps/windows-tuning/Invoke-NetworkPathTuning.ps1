<#
.SYNOPSIS
  Windows network tuning command for Network Lantern.

.DESCRIPTION
  Wraps the Windows tuning module with backup, apply, restore, and verify
  actions. Real mutation requires elevation and remains experimental.

.PARAMETER AllowUnsafeBackupFolder
  Override the backup-location pre-check for a path normally classified as a
  sensitive system directory. This does not bypass elevation or restore
  validation and is outside the supported alpha surface.
#>
[CmdletBinding()]
param(
  [ValidateSet('Apply', 'Backup', 'Restore', 'Verify')]
  [string]$Action = 'Apply',

  [ValidateSet('Safe', 'Measured')]
  [string]$TuningProfile = 'Safe',

  [ValidateRange(0, 63)]
  [sbyte]$Dscp = 46,

  [uint16[]]$UdpPorts = @(),

  [switch]$IncludeAppPolicies,

  [string[]]$AppPaths = @(),

  [ValidateSet('None', 'HighPerformance')]
  [string]$PowerPlan = 'None',

  [string]$BackupFolder,

  [switch]$AllowUnsafeBackupFolder,

  [switch]$PassThru,

  [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$manifestPath = Join-Path $repoRoot 'src/powershell/windows-tuning/NetworkLantern.WindowsTuning/NetworkLantern.WindowsTuning.psd1'
Import-Module -Name $manifestPath -Force

if (-not $PSBoundParameters.ContainsKey('BackupFolder')) {
  $defaultBackupFolder = Get-NetworkLanternDefaultBackupFolder
  if ($Action -eq 'Restore') {
    $manifestName = 'backup_manifest.json'
    $legacyBackupFolder = Join-Path -Path (Split-Path -Parent $defaultBackupFolder) -ChildPath 'NetworkDiagnosticsSuite'
    $newManifestPath = Join-Path -Path $defaultBackupFolder -ChildPath $manifestName
    $legacyManifestPath = Join-Path -Path $legacyBackupFolder -ChildPath $manifestName
    if (-not (Test-Path -LiteralPath $newManifestPath -PathType Leaf) -and
        (Test-Path -LiteralPath $legacyManifestPath -PathType Leaf)) {
      $defaultBackupFolder = $legacyBackupFolder
    }
  }
  $PSBoundParameters['BackupFolder'] = $defaultBackupFolder
}

$callerRequestedPassThru = [bool]$PassThru
$PSBoundParameters['PassThru'] = $true
$result = Invoke-NetworkPathTuning @PSBoundParameters

if ($callerRequestedPassThru) {
  $result
}

if ($null -eq $result -or -not [bool]$result.Success) {
  exit 1
}
