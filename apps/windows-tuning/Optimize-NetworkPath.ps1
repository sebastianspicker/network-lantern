<#
.SYNOPSIS
  Conservative Windows tuning workflow for network-diagnostics-suite.

.DESCRIPTION
  Wraps the Windows tuning module with a reduced, evidence-backed surface:
  backup, apply, restore, and verify. Tuning stays optional and reversible.
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

  [switch]$DryRun,

  [switch]$SkipAdminCheck
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$manifestPath = Join-Path $repoRoot 'src/powershell/windows-tuning/WindowsUdpJitterOptimization/WindowsUdpJitterOptimization.psd1'
Import-Module -Name $manifestPath -Force

if (-not $PSBoundParameters.ContainsKey('BackupFolder')) {
  $PSBoundParameters['BackupFolder'] = Get-NdsDefaultBackupFolder
}

Invoke-NetworkPathTuning @PSBoundParameters
