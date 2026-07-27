<#
.SYNOPSIS
Checks local verification prerequisites without installing or changing anything.

.PARAMETER IncludeIperf3
Also require iperf3 for live throughput runs.
#>

[CmdletBinding()]
param(
  [switch]$IncludeIperf3
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$checks = [System.Collections.Generic.List[object]]::new()

function Add-PrerequisiteCheck {
  param(
    [Parameter(Mandatory)][string]$Name,
    [Parameter(Mandatory)][bool]$Available,
    [Parameter(Mandatory)][string]$Detail
  )

  $checks.Add([pscustomobject]@{
      Name = $Name
      Available = $Available
      Detail = $Detail
    })
}

$powerShellReady = $PSVersionTable.PSVersion.Major -ge 7
Add-PrerequisiteCheck -Name 'PowerShell 7+' -Available $powerShellReady -Detail $PSVersionTable.PSVersion.ToString()

foreach ($commandName in @('git', 'bash', 'shellcheck', 'bats', 'jq')) {
  $command = Get-Command -Name $commandName -ErrorAction SilentlyContinue
  $detail = if ($command) { $command.Source } else { 'not found on PATH' }
  Add-PrerequisiteCheck -Name $commandName -Available ([bool]$command) -Detail $detail
}

foreach ($moduleRequirement in @(
    [pscustomobject]@{ Name = 'PSScriptAnalyzer'; Version = [version]'1.24.0' }
    [pscustomobject]@{ Name = 'Pester'; Version = [version]'5.7.1' }
  )) {
  $module = Get-Module -ListAvailable -Name $moduleRequirement.Name |
    Where-Object { $_.Version -eq $moduleRequirement.Version } |
    Select-Object -First 1
  $detail = if ($module) { $module.Version.ToString() } else { "required version $($moduleRequirement.Version) not found" }
  Add-PrerequisiteCheck -Name $moduleRequirement.Name -Available ([bool]$module) -Detail $detail
}

if ($IncludeIperf3) {
  $iperf3 = Get-Command -Name 'iperf3' -ErrorAction SilentlyContinue
  $detail = if ($iperf3) { $iperf3.Source } else { 'not found on PATH' }
  Add-PrerequisiteCheck -Name 'iperf3' -Available ([bool]$iperf3) -Detail $detail
}

foreach ($check in $checks) {
  $status = if ($check.Available) { 'OK' } else { 'MISSING' }
  Write-Output "[$status] $($check.Name): $($check.Detail)"
}

$missing = @($checks | Where-Object { -not $_.Available })
if ($missing.Count -gt 0) {
  [Console]::Error.WriteLine("Missing prerequisites: $($missing.Name -join ', ')")
  exit 1
}

Write-Output 'All requested prerequisites are available.'
exit 0
