<#
.SYNOPSIS
  Reports local tool and module readiness without installing anything.
#>
[CmdletBinding()]
param(
  [switch]$IncludeIperf3
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$requiredModules = @{
  PSScriptAnalyzer = '1.24.0'
  Pester           = '5.7.1'
}

$requiredCommands = @('pwsh', 'bash', 'git', 'shellcheck', 'bats')
if ($IncludeIperf3) {
  $requiredCommands += 'iperf3'
}

$results = New-Object System.Collections.Generic.List[object]

foreach ($commandName in $requiredCommands) {
  $command = Get-Command -Name $commandName -ErrorAction SilentlyContinue
  $results.Add([pscustomobject]@{
    Type     = 'Command'
    Name     = $commandName
    Required = 'present'
    Found    = [bool]$command
    Version  = if ($command -and $command.Version) { $command.Version.ToString() } else { '' }
    Path     = if ($command) { $command.Source } else { '' }
  })
}

foreach ($entry in $requiredModules.GetEnumerator() | Sort-Object Key) {
  $module = Get-Module -ListAvailable -Name $entry.Key |
    Where-Object { $_.Version -eq [version]$entry.Value } |
    Select-Object -First 1

  $newestModule = Get-Module -ListAvailable -Name $entry.Key |
    Sort-Object Version -Descending |
    Select-Object -First 1

  $results.Add([pscustomobject]@{
    Type     = 'PowerShellModule'
    Name     = $entry.Key
    Required = $entry.Value
    Found    = [bool]$module
    Version  = if ($module) { $module.Version.ToString() } elseif ($newestModule) { "found $($newestModule.Version)" } else { '' }
    Path     = if ($module) { $module.Path } elseif ($newestModule) { $newestModule.Path } else { '' }
  })
}

$results | Format-Table -AutoSize

$missing = @($results | Where-Object { -not $_.Found })
if ($missing.Count -gt 0) {
  [Console]::Error.WriteLine("Missing prerequisite(s): $($missing.Name -join ', ')")
  exit 1
}
