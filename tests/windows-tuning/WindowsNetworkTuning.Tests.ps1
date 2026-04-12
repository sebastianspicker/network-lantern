Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ManifestPath = Join-Path -Path $PSScriptRoot -ChildPath '../../src/powershell/windows-tuning/WindowsUdpJitterOptimization/WindowsUdpJitterOptimization.psd1'
Import-Module -Name $ManifestPath -Force

Describe 'Windows network tuning module' {
  It 'exports Invoke-NetworkPathTuning' {
    (Get-Command -Name Invoke-NetworkPathTuning -ErrorAction Stop).CommandType | Should -Be 'Function'
  }

  It 'exports Get-NdsDefaultBackupFolder' {
    (Get-Command -Name Get-NdsDefaultBackupFolder -ErrorAction Stop).CommandType | Should -Be 'Function'
  }

  It 'Get-NdsDefaultBackupFolder returns a suite-named folder' {
    $path = Get-NdsDefaultBackupFolder
    $path | Should -Not -BeNullOrEmpty
    $path | Should -Match 'NetworkDiagnosticsSuite$'
  }

  Context 'DryRun safety' {
    InModuleScope WindowsUdpJitterOptimization {
      It 'Apply DryRun returns a structured result for Safe profile' {
        Mock -CommandName Backup-UjState
        Mock -CommandName Resolve-UjRestoreStatus { 'OK' }
        Mock -CommandName Enable-UjLocalQosMarking
        Mock -CommandName New-UjDscpPolicyByPort

        $result = Invoke-NetworkPathTuning -Action Apply -TuningProfile Safe -UdpPorts @(5201, 5202) -DryRun -SkipAdminCheck -PassThru

        $result | Should -Not -BeNullOrEmpty
        $result.Action | Should -Be 'Apply'
        $result.TuningProfile | Should -Be 'Safe'
        $result.DryRun | Should -BeTrue
        $result.Success | Should -BeTrue
        $result.UdpPorts.Count | Should -Be 2
      }

      It 'Measured profile applies tier-1 NIC tuning and power plan in DryRun' {
        Mock -CommandName Backup-UjState
        Mock -CommandName Resolve-UjRestoreStatus { 'OK' }
        Mock -CommandName Enable-UjLocalQosMarking
        Mock -CommandName Set-UjNicConfiguration
        Mock -CommandName Set-UjPowerPlan

        $result = Invoke-NetworkPathTuning -Action Apply -TuningProfile Measured -DryRun -SkipAdminCheck -PassThru

        $result.Components['NicPowerSaving'] | Should -Be 'Skipped'
        $result.Components['PowerPlan'] | Should -Be 'Skipped'
        Assert-MockCalled -CommandName Set-UjNicConfiguration -Times 1 -Exactly
        Assert-MockCalled -CommandName Set-UjPowerPlan -Times 1 -Exactly
      }

      It 'blocks unsafe backup folder by default' {
        {
          Invoke-NetworkPathTuning -Action Backup -BackupFolder 'C:\Windows\System32\NetworkDiagnosticsSuite' -SkipAdminCheck -DryRun
        } | Should -Throw '*unsafe*'
      }
    }
  }

  Context 'Verify mode' {
    InModuleScope WindowsUdpJitterOptimization {
      It 'reports missing QoS policies' {
        Mock -CommandName Get-UjManagedQosPolicy { @([pscustomobject]@{ Name = 'NDS_QOS_PORT_5201' }) }
        Mock -CommandName Get-ItemProperty { [pscustomobject]@{ 'Do not use NLA' = '1' } }

        $result = Invoke-NetworkPathTuning -Action Verify -UdpPorts @(5201, 5202) -DryRun -PassThru

        $result.Success | Should -BeFalse
        $result.MissingPorts | Should -Contain 5202
      }
    }
  }

  It 'CLI wrapper resolves default backup folder via module function' {
    $scriptPath = Join-Path -Path $PSScriptRoot -ChildPath '../../apps/windows-tuning/Optimize-NetworkPath.ps1'
    $result = & $scriptPath -Action Backup -DryRun -SkipAdminCheck -PassThru

    $result | Should -Not -BeNullOrEmpty
    $result.BackupFolder | Should -Be (Get-NdsDefaultBackupFolder)
  }
}
