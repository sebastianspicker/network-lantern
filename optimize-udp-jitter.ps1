<#
.SYNOPSIS
  Reduces UDP jitter (lag spikes, choppy voice) on Windows 10/11 for gaming and VoIP.

.DESCRIPTION
  Applies network optimizations to reduce packet timing inconsistency (jitter) that causes
  lag spikes, rubber-banding, and choppy voice chat in games like CS2 and apps like TeamSpeak.

  This script imports the WindowsUdpJitterOptimization module and provides four actions:
  - Apply: Apply a preset of optimizations (your current settings are backed up first).
  - Backup: Save your current settings without making changes.
  - Restore: Revert to your previously backed-up settings.
  - ResetDefaults: Remove all optimizations and restore stock Windows behavior.

.PARAMETER Action
  What to do: Apply (optimize), Backup (save current state), Restore (undo), or ResetDefaults (factory reset).

.PARAMETER Preset
  Optimization level (Apply only):
  1 = Safe: QoS priority tagging + power-saving disables. Zero risk.
  2 = Moderate: Adds interrupt and flow control tuning. Slightly higher CPU usage.
  3 = Aggressive: Maximum optimization. Noticeably higher CPU usage.

.PARAMETER AllowUnsafeBackupFolder
  Allows backup/restore/apply paths under sensitive system directories.

.PARAMETER PassThru
  Returns a structured result object for automation.

.PARAMETER SkipAdminCheck
  Skip the administrator privilege check (e.g. for testing or constrained environments).

.EXAMPLE
  .\optimize-udp-jitter.ps1 -Action Apply -Preset 2 -WhatIf

.EXAMPLE
  .\optimize-udp-jitter.ps1 -Action Backup -BackupFolder C:\MyBackup

.NOTES
  Author: Sebastian J. Spicker
  License: MIT
#>
[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
param(
  [ValidateSet('Apply', 'Backup', 'Restore', 'ResetDefaults')]
  [string]$Action = 'Apply',

  [ValidateSet(1, 2, 3)]
  [int]$Preset = 1,

  [ValidateRange(1, 65535)]
  [int]$TeamSpeakPort = 9987,

  [ValidateRange(1, 65535)]
  [int]$CS2PortStart = 27015,

  [ValidateRange(1, 65535)]
  [int]$CS2PortEnd = 27036,

  [switch]$IncludeAppPolicies,

  [string[]]$AppPaths = @(),

  [ValidateRange(0, 65535)]
  [int]$AfdThreshold = 1500,

  [ValidateSet('None', 'HighPerformance', 'Ultimate')]
  [string]$PowerPlan = 'None',

  [switch]$DisableGameDvr,

  [switch]$DisableUro,

  [switch]$IncludeExperimental,

  [string]$BackupFolder,

  [switch]$AllowUnsafeBackupFolder,

  [switch]$PassThru,

  [switch]$DryRun,

  [switch]$SkipAdminCheck
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$manifestPath = Join-Path -Path $PSScriptRoot -ChildPath 'WindowsUdpJitterOptimization/WindowsUdpJitterOptimization.psd1'
Import-Module -Name $manifestPath -Force

if (-not $PSBoundParameters.ContainsKey('BackupFolder')) {
  $PSBoundParameters['BackupFolder'] = Get-UjDefaultBackupFolder
}

Invoke-UdpJitterOptimization @PSBoundParameters
