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

  It 'does not export legacy tuning entrypoints' {
    { Get-Command -Name Invoke-UdpJitterOptimization -ErrorAction Stop } | Should -Throw
    { Get-Command -Name Get-UjDefaultBackupFolder -ErrorAction Stop } | Should -Throw
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

      It 'does not report success when QoS verification is unavailable' {
        Mock -CommandName Get-Command {
          param([string]$Name)
          if ($Name -eq 'Get-NetQosPolicy') { return $null }
        }
        Mock -CommandName Get-ItemProperty { [pscustomobject]@{ 'Do not use NLA' = '1' } }

        $result = Invoke-NetworkPathTuning -Action Verify -UdpPorts @(5201) -DryRun -PassThru

        $result.Success | Should -BeFalse
        $result.Components['QosPolicies'] | Should -Be 'Unknown'
        @($result.Warnings).Count | Should -BeGreaterThan 0
      }
    }
  }

  Context 'Apply helper failure status' {
    InModuleScope WindowsUdpJitterOptimization {
      It 'New-UjDscpPolicyByPort returns false when policy creation fails' {
        function New-NetQosPolicy { throw 'policy limit' }
        Mock -CommandName Get-UjManagedQosPolicy { @() }
        Mock -CommandName New-NetQosPolicy { throw 'policy limit' }

        $result = New-UjDscpPolicyByPort -Name 'NDS_QOS_PORT_5201' -PortStart 5201 -PortEnd 5201 -Confirm:$false

        $result | Should -BeOfType [bool]
        $result | Should -BeFalse
      }

      It 'New-UjDscpPolicyByApp returns false when policy creation fails' {
        function New-NetQosPolicy { throw 'policy limit' }
        Mock -CommandName Get-UjManagedQosPolicy { @() }
        Mock -CommandName New-NetQosPolicy { throw 'policy limit' }

        $result = New-UjDscpPolicyByApp -Name 'NDS_QOS_APP_1' -ExePath 'C:\app\test.exe' -Confirm:$false

        $result | Should -BeOfType [bool]
        $result | Should -BeFalse
      }

      It 'Set-UjPowerPlan returns false when powercfg cannot switch plans' {
        function powercfg {
          $global:LASTEXITCODE = 1
          'failed'
        }

        $result = Set-UjPowerPlan -PowerPlan HighPerformance -Confirm:$false

        $result | Should -BeOfType [bool]
        $result | Should -BeFalse
      }

      It 'Set-UjNicConfiguration returns false when adapters cannot be detected' {
        $result = Set-UjNicConfiguration -Preset 1 -Confirm:$false

        $result | Should -BeOfType [bool]
        $result | Should -BeFalse
      }
    }
  }

  Context 'Restore safety and reset scope' {
    InModuleScope WindowsUdpJitterOptimization {
      It 'rejects missing backup manifests before restore work starts' {
        $backupFolder = Join-Path $TestDrive 'missing-manifest-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null

        Mock -CommandName Restore-UjRegistryFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjQosFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjNicFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjRscFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjPowerPlanFromBackup { throw 'should not run' }

        $result = Restore-UjState -BackupFolder $backupFolder

        $result['Manifest'] | Should -Be 'Warn'
        $result['Registry'] | Should -Be 'Skipped'
      }

      It 'rejects invalid backup manifests before restore work starts' {
        $backupFolder = Join-Path $TestDrive 'invalid-manifest-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Value '{' -Encoding UTF8

        Mock -CommandName Restore-UjRegistryFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjQosFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjNicFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjRscFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjPowerPlanFromBackup { throw 'should not run' }

        $result = Restore-UjState -BackupFolder $backupFolder

        $result['Manifest'] | Should -Be 'Warn'
        $result['Registry'] | Should -Be 'Skipped'
      }

      It 'rejects incompatible backup manifests before restore work starts' {
        $backupFolder = Join-Path $TestDrive 'incompatible-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        @{
          SchemaVersion = 999
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        Mock -CommandName Restore-UjRegistryFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjQosFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjNicFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjRscFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjPowerPlanFromBackup { throw 'should not run' }

        $result = Restore-UjState -BackupFolder $backupFolder

        $result['Manifest'] | Should -Be 'Warn'
        $result['Registry'] | Should -Be 'Skipped'
      }

      It 'rejects restore manifests without artifact digests before restore work starts' {
        $backupFolder = Join-Path $TestDrive 'unsigned-manifest-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{}
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        Mock -CommandName Restore-UjRegistryFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjQosFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjNicFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjRscFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjPowerPlanFromBackup { throw 'should not run' }

        $result = Restore-UjState -BackupFolder $backupFolder

        $result['Manifest'] | Should -Be 'Warn'
        $result['Registry'] | Should -Be 'Skipped'
      }

      It 'accepts restore manifests when listed artifact digests match' {
        $backupFolder = Join-Path $TestDrive 'digested-manifest-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        $powerPlanPath = Join-Path $backupFolder $script:UjBackupFilePowerplan
        Set-Content -LiteralPath $powerPlanPath -Value $script:UjPowerPlanGuidBalanced -Encoding UTF8
        $digests = @{
          $script:UjBackupFilePowerplan = (Get-FileHash -LiteralPath $powerPlanPath -Algorithm SHA256).Hash
        }
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{ PowerPlan = $true }
          ArtifactDigests = $digests
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        Mock -CommandName Restore-UjRegistryFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjQosFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjNicFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjRscFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjPowerPlanFromBackup { Get-UjRestoreComponentResult -Status 'OK' }

        $result = Restore-UjState -BackupFolder $backupFolder

        $result['PowerPlan'] | Should -Be 'OK'
        Assert-MockCalled -CommandName Restore-UjPowerPlanFromBackup -Times 1 -Exactly
      }

      It 'rejects untrusted restore paths before artifact digests or restore work' {
        $backupFolder = Join-Path $TestDrive 'untrusted-path-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{}
          ArtifactDigests = @{}
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        Mock -CommandName Test-UjBackupPathTrust {
          [pscustomobject]@{ IsTrusted = $false; Message = 'Backup path owner is not trusted: test' }
        }
        Mock -CommandName Test-UjBackupArtifactDigests { throw 'should not run' }
        Mock -CommandName Restore-UjRegistryFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjQosFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjNicFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjRscFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjPowerPlanFromBackup { throw 'should not run' }

        $result = Restore-UjState -BackupFolder $backupFolder

        $result['Manifest'] | Should -Be 'Warn'
        Assert-MockCalled -CommandName Test-UjBackupArtifactDigests -Times 0 -Exactly
      }

      It 'rejects tampered restore artifacts before restore work starts' {
        $backupFolder = Join-Path $TestDrive 'tampered-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        $powerPlanPath = Join-Path $backupFolder $script:UjBackupFilePowerplan
        Set-Content -LiteralPath $powerPlanPath -Value $script:UjPowerPlanGuidBalanced -Encoding UTF8
        $digests = @{
          $script:UjBackupFilePowerplan = (Get-FileHash -LiteralPath $powerPlanPath -Algorithm SHA256).Hash
        }
        Set-Content -LiteralPath $powerPlanPath -Value $script:UjPowerPlanGuidHighPerformance -Encoding UTF8
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{ PowerPlan = $true }
          ArtifactDigests = $digests
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        Mock -CommandName Restore-UjRegistryFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjQosFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjNicFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjRscFromBackup { throw 'should not run' }
        Mock -CommandName Restore-UjPowerPlanFromBackup { throw 'should not run' }

        $result = Restore-UjState -BackupFolder $backupFolder

        $result['Manifest'] | Should -Be 'Warn'
        $result['PowerPlan'] | Should -Be 'Skipped'
      }

      It 'reset removes only owned MMCSS audio values' {
        function Get-NetTCPSetting {}
        $script:UjRegistryPathSystemProfile = '/tmp/SystemProfile'
        $script:UjRegistryPathAfdParameters = '/tmp/AfdParameters'
        $script:UjRegistryPathQos = '/tmp/Qos'
        Mock -CommandName Set-UjPowerPlan {}
        Mock -CommandName Set-UjGameDvrState {}
        Mock -CommandName Remove-UjManagedQosPolicy {}
        Mock -CommandName Get-NetTCPSetting { $null }
        Mock -CommandName Test-Path { $true }
        Mock -CommandName Remove-ItemProperty {}
        Mock -CommandName Remove-Item {}

        Reset-UjBaseline -Confirm:$false

        Assert-MockCalled -CommandName Remove-Item -Times 0 -Exactly
        Assert-MockCalled -CommandName Remove-ItemProperty -Times 6
      }
    }
  }

  It 'CLI wrapper resolves default backup folder via module function' {
    $scriptPath = Join-Path -Path $PSScriptRoot -ChildPath '../../apps/windows-tuning/Optimize-NetworkPath.ps1'
    $result = & $scriptPath -Action Backup -DryRun -SkipAdminCheck -PassThru

    $result | Should -Not -BeNullOrEmpty
    $result.BackupFolder | Should -Be (Get-NdsDefaultBackupFolder)
  }

  It 'GUI entrypoint is informational instead of failing' {
    $scriptPath = Join-Path -Path $PSScriptRoot -ChildPath '../../apps/windows-tuning/Optimize-NetworkPath-GUI.ps1'
    $output = & pwsh -NoLogo -NoProfile -File $scriptPath 2>&1

    $LASTEXITCODE | Should -Be 0
    $message = $output | Out-String
    $message | Should -Match 'CLI-first'
    $message | Should -Match 'compatibility entrypoint'
    $message | Should -Match 'does not launch a GUI'
  }

  Context 'Backup action' {
    InModuleScope WindowsUdpJitterOptimization {
      It 'Backup action with PassThru returns a structured result' {
        Mock -CommandName Backup-UjState {
          [pscustomobject]@{ Status = 'OK'; Message = 'Backup complete.' }
        }
        Mock -CommandName Resolve-UjRestoreStatus { 'OK' }

        $result = Invoke-NetworkPathTuning -Action Backup -DryRun -SkipAdminCheck -PassThru

        $result | Should -Not -BeNullOrEmpty
        $result.Action | Should -Be 'Backup'
        $result.DryRun | Should -BeTrue
        $result.Success | Should -BeTrue
        $result.Components['Backup'] | Should -Be 'OK'
      }

      It 'marks backup incomplete when QoS policies cannot be read' {
        $backupFolder = Join-Path $TestDrive 'qos-read-failure-backup'
        try {
          [Microsoft.PowerShell.Cmdletization.GeneratedTypes.NetAdapter.NetAdapter] | Out-Null
        } catch {
          Add-Type -TypeDefinition @'
namespace Microsoft.PowerShell.Cmdletization.GeneratedTypes.NetAdapter {
  public class NetAdapter {}
}
'@
        }
        function Get-NetAdapterRsc {}
        function powercfg {
          $global:LASTEXITCODE = 1
          @()
        }

        Mock -CommandName Export-UjRegistryKey { $true }
        Mock -CommandName Get-UjManagedQosPolicy {
          param([switch]$ErrorOnFailure)
          if ($ErrorOnFailure) {
            throw 'qos read failed'
          }
          @()
        }
        Mock -CommandName Get-UjPhysicalUpAdapter { @() }
        Mock -CommandName Get-NetAdapterRsc { @() }

        $result = Backup-UjState -BackupFolder $backupFolder -Confirm:$false
        $manifest = Get-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Raw |
          ConvertFrom-Json

        $result.Status | Should -Be 'Warn'
        $manifest.Components.QosPolicies | Should -BeFalse
      }
    }
  }

  Context 'Restore action' {
    InModuleScope WindowsUdpJitterOptimization {
      It 'Restore action with a valid manifest succeeds when all components report OK' {
        Mock -CommandName Restore-UjState {
          @{ Registry = 'OK'; QosPolicies = 'OK'; Manifest = 'OK' }
        }

        $result = Invoke-NetworkPathTuning -Action Restore -DryRun -SkipAdminCheck -PassThru

        $result | Should -Not -BeNullOrEmpty
        $result.Action | Should -Be 'Restore'
        $result.Success | Should -BeTrue
        $result.Components['Registry'] | Should -Be 'OK'
        $result.Components['QosPolicies'] | Should -Be 'OK'
      }
    }
  }

  Context 'IncludeAppPolicies flag' {
    InModuleScope WindowsUdpJitterOptimization {
      It 'IncludeAppPolicies with AppPaths reaches New-UjDscpPolicyByApp' {
        Mock -CommandName Backup-UjState
        Mock -CommandName Resolve-UjRestoreStatus { 'OK' }
        Mock -CommandName Enable-UjLocalQosMarking
        Mock -CommandName New-UjDscpPolicyByPort
        Mock -CommandName New-UjDscpPolicyByApp

        $result = Invoke-NetworkPathTuning `
          -Action Apply `
          -IncludeAppPolicies `
          -AppPaths @('C:\app\test.exe') `
          -DryRun `
          -SkipAdminCheck `
          -PassThru

        Assert-MockCalled -CommandName New-UjDscpPolicyByApp -Times 1 -Exactly
        $result.IncludeAppPolicies | Should -BeTrue
        $result.AppPaths | Should -Contain 'C:\app\test.exe'
      }
    }
  }

  Context 'Non-Windows safety guard' {
    It 'throws on non-Windows without -DryRun' {
      $runningOnWindows = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
        [System.Runtime.InteropServices.OSPlatform]::Windows
      )
      if ($runningOnWindows) {
        Set-ItResult -Skipped -Because 'running on Windows; non-Windows guard cannot be exercised'
        return
      }

      { Invoke-NetworkPathTuning -Action Apply -SkipAdminCheck } | Should -Throw '*Windows*'
    }
  }

  Context 'Backup manifest metadata' {
    InModuleScope WindowsUdpJitterOptimization {
      It 'Get-UjBackupManifestMetadata returns required enrichment fields' {
        $metadata = Get-UjBackupManifestMetadata

        $metadata | Should -Not -BeNullOrEmpty
        $metadata['SchemaVersion'] | Should -BeGreaterThan 0
        $metadata['ToolName'] | Should -Be 'network-diagnostics-suite'
        $metadata['MachineName'] | Should -Not -BeNullOrEmpty
        $metadata['Platform'] | Should -Not -BeNullOrEmpty
        $metadata['OsVersion'] | Should -Not -BeNullOrEmpty
      }
    }
  }
}
