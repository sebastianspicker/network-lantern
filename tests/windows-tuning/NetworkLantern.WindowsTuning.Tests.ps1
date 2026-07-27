Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ManifestPath = Join-Path -Path $PSScriptRoot -ChildPath '../../src/powershell/windows-tuning/NetworkLantern.WindowsTuning/NetworkLantern.WindowsTuning.psd1'
Import-Module -Name $ManifestPath -Force

Describe 'Windows network tuning module' {
  It 'exports Invoke-NetworkPathTuning' {
    (Get-Command -Name Invoke-NetworkPathTuning -ErrorAction Stop).CommandType | Should -Be 'Function'
  }

  It 'does not expose an administrator-check bypass' {
    (Get-Command -Name Invoke-NetworkPathTuning -ErrorAction Stop).Parameters.Keys |
      Should -Not -Contain 'SkipAdminCheck'
  }

  It 'exports Get-NetworkLanternDefaultBackupFolder' {
    (Get-Command -Name Get-NetworkLanternDefaultBackupFolder -ErrorAction Stop).CommandType | Should -Be 'Function'
  }

  It 'does not export legacy tuning entrypoints' {
    { Get-Command -Name Invoke-UdpJitterOptimization -ErrorAction Stop } | Should -Throw
    { Get-Command -Name Get-UjDefaultBackupFolder -ErrorAction Stop } | Should -Throw
  }

  It 'Get-NetworkLanternDefaultBackupFolder returns a suite-named folder' {
    $path = Get-NetworkLanternDefaultBackupFolder
    $path | Should -Not -BeNullOrEmpty
    $path | Should -Match 'NetworkLantern$'
  }

  Context 'DryRun safety' {
    InModuleScope 'NetworkLantern.WindowsTuning' {
      It 'Apply DryRun returns a structured result for Safe profile' {
        Mock -CommandName Assert-UjAdministrator { throw 'administrator check must not run during DryRun' }
        Mock -CommandName Backup-UjState
        Mock -CommandName Resolve-UjRestoreStatus { 'OK' }
        Mock -CommandName Enable-UjLocalQosMarking
        Mock -CommandName New-UjDscpPolicyByPort

        $result = Invoke-NetworkPathTuning -Action Apply -TuningProfile Safe -UdpPorts @(5201, 5202) -DryRun -PassThru

        $result | Should -Not -BeNullOrEmpty
        $result.Action | Should -Be 'Apply'
        $result.TuningProfile | Should -Be 'Safe'
        $result.DryRun | Should -BeTrue
        $result.Success | Should -BeTrue
        $result.UdpPorts.Count | Should -Be 2
        Should -Invoke -CommandName New-UjDscpPolicyByPort -Times 1 -Exactly -ParameterFilter {
          $Name -eq 'NETWORK_LANTERN_QOS_PORT_5201'
        }
        Should -Invoke -CommandName New-UjDscpPolicyByPort -Times 1 -Exactly -ParameterFilter {
          $Name -eq 'NETWORK_LANTERN_QOS_PORT_5202'
        }
        Should -Invoke -CommandName Assert-UjAdministrator -Times 0 -Exactly
      }

      It 'Measured profile applies tier-1 NIC tuning and power plan in DryRun' {
        Mock -CommandName Backup-UjState
        Mock -CommandName Resolve-UjRestoreStatus { 'OK' }
        Mock -CommandName Enable-UjLocalQosMarking
        Mock -CommandName Set-UjNicConfiguration
        Mock -CommandName Set-UjPowerPlan

        $result = Invoke-NetworkPathTuning -Action Apply -TuningProfile Measured -DryRun -PassThru

        $result.Components['NicPowerSaving'] | Should -Be 'Skipped'
        $result.Components['PowerPlan'] | Should -Be 'Skipped'
        Assert-MockCalled -CommandName Set-UjNicConfiguration -Times 1 -Exactly
        Assert-MockCalled -CommandName Set-UjPowerPlan -Times 1 -Exactly
      }

      It 'blocks unsafe backup folder by default' {
        {
          Invoke-NetworkPathTuning -Action Backup -BackupFolder 'C:\Windows\System32\NetworkDiagnosticsSuite' -DryRun
        } | Should -Throw '*unsafe*'
      }
    }
  }

  Context 'Verify mode' {
    InModuleScope 'NetworkLantern.WindowsTuning' {
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
    InModuleScope 'NetworkLantern.WindowsTuning' {
      It 'New-UjDscpPolicyByPort returns false when policy creation fails' {
        function New-NetQosPolicy { throw 'policy limit' }
        Mock -CommandName Get-UjManagedQosPolicy { @() }
        Mock -CommandName New-NetQosPolicy { throw 'policy limit' }

        $result = New-UjDscpPolicyByPort -Name 'NETWORK_LANTERN_QOS_PORT_5201' -PortStart 5201 -PortEnd 5201 -Confirm:$false

        $result | Should -BeOfType [bool]
        $result | Should -BeFalse
      }

      It 'New-UjDscpPolicyByApp returns false when policy creation fails' {
        function New-NetQosPolicy { throw 'policy limit' }
        Mock -CommandName Get-UjManagedQosPolicy { @() }
        Mock -CommandName New-NetQosPolicy { throw 'policy limit' }

        $result = New-UjDscpPolicyByApp -Name 'NETWORK_LANTERN_QOS_APP_1' -ExePath 'C:\app\test.exe' -Confirm:$false

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

      It 'Set-UjNicConfiguration returns false when no active physical adapter exists' {
        try {
          [Microsoft.PowerShell.Cmdletization.GeneratedTypes.NetAdapter.NetAdapter] | Out-Null
        } catch {
          Add-Type -TypeDefinition @'
namespace Microsoft.PowerShell.Cmdletization.GeneratedTypes.NetAdapter {
  public class NetAdapter {}
}
'@
        }
        Mock -CommandName Get-UjPhysicalUpAdapter { @() }

        $result = Set-UjNicConfiguration -Preset 1 -Confirm:$false

        $result | Should -BeOfType [bool]
        $result | Should -BeFalse
      }
    }
  }

  Context 'Apply backup verification' {
    InModuleScope 'NetworkLantern.WindowsTuning' {
      It 'refuses mutation when the backup status is incomplete' {
        if (-not [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)) {
          Set-ItResult -Skipped -Because 'the public Apply path refuses non-Windows mutation before backup verification'
          return
        }

        Mock -CommandName Backup-UjState { [pscustomobject]@{ Status = 'Warn'; Message = 'QoS backup failed.' } }
        Mock -CommandName Resolve-UjRestoreStatus { 'Warn' }
        Mock -CommandName Read-UjBackupManifest { [pscustomobject]@{ Status = 'Invalid'; Message = 'Backup artifact digest mismatch.' } }
        Mock -CommandName Enable-UjLocalQosMarking { throw 'must not mutate' }
        Mock -CommandName Assert-UjAdministrator

        { Invoke-NetworkPathTuning -Action Apply -BackupFolder $TestDrive } |
          Should -Throw '*backup was not verified*'

        Assert-MockCalled -CommandName Read-UjBackupManifest -Times 1 -Exactly
        Assert-MockCalled -CommandName Enable-UjLocalQosMarking -Times 0 -Exactly
      }
    }
  }

  Context 'Restore safety and reset scope' {
    InModuleScope 'NetworkLantern.WindowsTuning' {
      BeforeEach {
        $runningOnWindows = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
          [System.Runtime.InteropServices.OSPlatform]::Windows
        )
        if ($runningOnWindows) {
          # Pester's Windows TestDrive inherits the host TEMP ACL, which can be
          # writable by unrelated identities. Give source fixtures a trusted
          # DACL and redirect privileged staging to the disposable test root.
          $testDriveAcl = [System.Security.AccessControl.DirectorySecurity]::new()
          $testDriveAcl.SetAccessRuleProtection($true, $false)
          $inheritance = [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor
            [System.Security.AccessControl.InheritanceFlags]::ObjectInherit
          $trustedFixtureSids = @(
            [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value,
            'S-1-5-18',
            'S-1-5-32-544'
          )
          foreach ($sidValue in $trustedFixtureSids) {
            $sid = [System.Security.Principal.SecurityIdentifier]::new($sidValue)
            $rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
              $sid,
              [System.Security.AccessControl.FileSystemRights]::FullControl,
              $inheritance,
              [System.Security.AccessControl.PropagationFlags]::None,
              [System.Security.AccessControl.AccessControlType]::Allow
            )
            $testDriveAcl.AddAccessRule($rule) | Out-Null
          }
          [System.IO.FileSystemAclExtensions]::SetAccessControl(
            [System.IO.DirectoryInfo]::new($TestDrive),
            $testDriveAcl
          )

          $script:UjRestoreFixtureRoot = $TestDrive
          Mock -CommandName Get-UjWindowsRestoreStagingRoot { $script:UjRestoreFixtureRoot }
          Mock -CommandName Initialize-UjAdminOnlyDirectory {
            param([string]$Path)
            return [System.IO.Directory]::CreateDirectory($Path).FullName
          }
          Mock -CommandName Protect-UjAdminOnlyFile
          Mock -CommandName Test-UjWindowsAdminOnlyPath {
            [pscustomobject]@{ IsTrusted = $true; Message = '' }
          }
        }
      }

      It 'uses a fixed administrative SID allow-list for privileged restore staging' {
        $trustedSids = @(Get-UjTrustedStagingSidValue)

        $trustedSids.Count | Should -Be 3
        $trustedSids | Should -Contain 'S-1-5-18'
        $trustedSids | Should -Contain 'S-1-5-32-544'
        $trustedSids | Should -Contain 'S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464'
        $trustedSids | Should -Not -Contain 'S-1-1-0'
        $trustedSids | Should -Not -Contain 'S-1-5-11'
        $trustedSids | Should -Not -Contain 'S-1-5-32-545'
      }

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

      It 'rejects a backup manifest without a Components map' {
        $backupFolder = Join-Path $TestDrive 'missing-components-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          ArtifactDigests = @{}
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        $result = Read-UjBackupManifest -BackupFolder $backupFolder

        $result.Status | Should -Be 'Invalid'
        $result.Message | Should -Match 'Components'
      }

      It 'rejects a backup manifest whose Components value is not a map' {
        $backupFolder = Join-Path $TestDrive 'nondictionary-components-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @('PowerPlan')
          ArtifactDigests = @{}
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        $result = Read-UjBackupManifest -BackupFolder $backupFolder

        $result.Status | Should -Be 'Invalid'
        $result.Message | Should -Match 'dictionary'
      }

      It 'rejects a backup manifest with an empty Components map' {
        $backupFolder = Join-Path $TestDrive 'literal-empty-components-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{}
          ArtifactDigests = @{}
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        $result = Read-UjBackupManifest -BackupFolder $backupFolder

        $result.Status | Should -Be 'Invalid'
        $result.Message | Should -Match 'missing component state'
      }

      It 'rejects a backup manifest with no enabled component' {
        $backupFolder = Join-Path $TestDrive 'empty-components-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{
            SystemProfile = $false
            AfdParameters = $false
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            PowerPlan = $false
          }
          ArtifactDigests = @{}
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        $result = Read-UjBackupManifest -BackupFolder $backupFolder

        $result.Status | Should -Be 'Invalid'
        $result.Message | Should -Match 'at least one'
      }

      It 'rejects a backup manifest with an unknown component' {
        $backupFolder = Join-Path $TestDrive 'unknown-component-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{
            SystemProfile = $false
            AfdParameters = $false
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            PowerPlan = $true
            ArbitraryCommand = $true
          }
          ArtifactDigests = @{}
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        $result = Read-UjBackupManifest -BackupFolder $backupFolder

        $result.Status | Should -Be 'Invalid'
        $result.Message | Should -Match 'unknown component'
      }

      It 'rejects a backup manifest whose component key casing is not exact' {
        $backupFolder = Join-Path $TestDrive 'wrong-case-component-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{
            SystemProfile = $false
            AfdParameters = $false
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            powerplan = $true
          }
          ArtifactDigests = @{}
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        $result = Read-UjBackupManifest -BackupFolder $backupFolder

        $result.Status | Should -Be 'Invalid'
        $result.Message | Should -Match 'unknown component'
      }

      It 'rejects a backup manifest whose top-level Components casing is not exact' {
        $backupFolder = Join-Path $TestDrive 'wrong-case-components-property-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        @'
{
  "SchemaVersion": 1,
  "ToolName": "network-diagnostics-suite",
  "Timestamp": "2026-01-01T00:00:00Z",
  "components": {
    "SystemProfile": false,
    "AfdParameters": false,
    "QosPolicies": false,
    "NicAdvanced": false,
    "NicRsc": false,
    "PowerPlan": true
  },
  "ArtifactDigests": {}
}
'@ | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        $result = Read-UjBackupManifest -BackupFolder $backupFolder

        $result.Status | Should -Be 'Invalid'
        $result.Message | Should -Match 'casing.*Components'
      }

      It 'rejects duplicate component keys before JSON hashtable conversion' {
        $backupFolder = Join-Path $TestDrive 'duplicate-component-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        @'
{
  "SchemaVersion": 1,
  "ToolName": "network-diagnostics-suite",
  "Timestamp": "2026-01-01T00:00:00Z",
  "Components": {
    "SystemProfile": false,
    "AfdParameters": false,
    "QosPolicies": false,
    "NicAdvanced": false,
    "NicRsc": false,
    "PowerPlan": true,
    "PowerPlan": false
  },
  "ArtifactDigests": {}
}
'@ | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        $result = Read-UjBackupManifest -BackupFolder $backupFolder

        $result.Status | Should -Be 'Invalid'
        $result.Message | Should -Match 'duplicate component.*PowerPlan'
      }

      It 'rejects case-variant component pairs before JSON hashtable conversion' {
        $backupFolder = Join-Path $TestDrive 'case-variant-component-pair-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        @'
{
  "SchemaVersion": 1,
  "ToolName": "network-diagnostics-suite",
  "Timestamp": "2026-01-01T00:00:00Z",
  "Components": {
    "SystemProfile": false,
    "AfdParameters": false,
    "QosPolicies": false,
    "NicAdvanced": false,
    "NicRsc": false,
    "PowerPlan": true,
    "powerplan": false
  },
  "ArtifactDigests": {}
}
'@ | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        $result = Read-UjBackupManifest -BackupFolder $backupFolder

        $result.Status | Should -Be 'Invalid'
        $result.Message | Should -Match 'duplicate component or case variant'
      }

      It 'rejects a backup manifest with a nonboolean component state' {
        $backupFolder = Join-Path $TestDrive 'nonboolean-component-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{
            SystemProfile = $false
            AfdParameters = $false
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            PowerPlan = 'true'
          }
          ArtifactDigests = @{}
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        $result = Read-UjBackupManifest -BackupFolder $backupFolder

        $result.Status | Should -Be 'Invalid'
        $result.Message | Should -Match 'boolean'
      }

      It 'rejects incompatible backup manifests before restore work starts' {
        $backupFolder = Join-Path $TestDrive 'incompatible-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        @{
          SchemaVersion = 999
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{
            SystemProfile = $false
            AfdParameters = $false
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            PowerPlan = $true
          }
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
        Set-Content -LiteralPath (Join-Path $backupFolder $script:UjBackupFilePowerplan) -Value $script:UjPowerPlanGuidBalanced -Encoding UTF8
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{
            SystemProfile = $false
            AfdParameters = $false
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            PowerPlan = $true
          }
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
          Components = @{
            SystemProfile = $false
            AfdParameters = $false
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            PowerPlan = $true
          }
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

      It 'rejects an enabled component when its expected artifact and digest are missing' {
        $backupFolder = Join-Path $TestDrive 'missing-enabled-artifact'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{
            SystemProfile = $false
            AfdParameters = $false
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            PowerPlan = $true
          }
          ArtifactDigests = @{}
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        $result = Read-UjBackupManifest -BackupFolder $backupFolder

        $result.Status | Should -Be 'Invalid'
        $result.Message | Should -Match 'powerplan\.txt'
      }

      It 'rejects an enabled component artifact when its digest is missing' {
        $backupFolder = Join-Path $TestDrive 'missing-enabled-digest'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        Set-Content -LiteralPath (Join-Path $backupFolder $script:UjBackupFilePowerplan) -Value $script:UjPowerPlanGuidBalanced -Encoding UTF8
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{
            SystemProfile = $false
            AfdParameters = $false
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            PowerPlan = $true
          }
          ArtifactDigests = @{}
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        $result = Read-UjBackupManifest -BackupFolder $backupFolder

        $result.Status | Should -Be 'Invalid'
        $result.Message | Should -Match 'digest.*powerplan\.txt|powerplan\.txt.*digest'
      }

      It 'reads and restores a verified backup from a folder containing wildcard characters' {
        $backupFolder = Join-Path $TestDrive 'verified-[literal]-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        $powerPlanPath = Join-Path $backupFolder $script:UjBackupFilePowerplan
        Set-Content -LiteralPath $powerPlanPath -Value $script:UjPowerPlanGuidBalanced -Encoding UTF8
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{
            SystemProfile = $false
            AfdParameters = $false
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            PowerPlan = $true
          }
          ArtifactDigests = @{
            $script:UjBackupFilePowerplan = (Get-FileHash -LiteralPath $powerPlanPath -Algorithm SHA256).Hash
          }
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        Mock -CommandName Restore-UjRegistryFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjQosFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjNicFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjRscFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjPowerPlanFromBackup { Get-UjRestoreComponentResult -Status 'OK' }

        $result = Restore-UjState -BackupFolder $backupFolder

        $result['PowerPlan'] | Should -Be 'OK'
      }

      It 'restores only from a freshly staged copy of verified artifacts' {
        $backupFolder = Join-Path $TestDrive 'staging-source-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        $powerPlanPath = Join-Path $backupFolder $script:UjBackupFilePowerplan
        Set-Content -LiteralPath $powerPlanPath -Value $script:UjPowerPlanGuidBalanced -Encoding UTF8
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{
            SystemProfile = $false
            AfdParameters = $false
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            PowerPlan = $true
          }
          ArtifactDigests = @{
            $script:UjBackupFilePowerplan = (Get-FileHash -LiteralPath $powerPlanPath -Algorithm SHA256).Hash
          }
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        $script:observedRestoreFolder = $null
        Mock -CommandName Assert-UjRestoreStagingConsumerInvariant {
          param($Session, [hashtable]$Manifest)
          Assert-UjRestoreStagingInvariant -Session $Session -Manifest $Manifest
        }
        Mock -CommandName Restore-UjRegistryFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjQosFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjNicFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjRscFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjPowerPlanFromBackup {
          param([string]$BackupFolder)
          $script:observedRestoreFolder = $BackupFolder
          Get-UjRestoreComponentResult -Status 'OK'
        }

        $result = Restore-UjState -BackupFolder $backupFolder

        $result['PowerPlan'] | Should -Be 'OK'
        $script:observedRestoreFolder | Should -Not -Be $backupFolder
        $script:observedRestoreFolder | Should -Not -BeNullOrEmpty
        Assert-MockCalled -CommandName Assert-UjRestoreStagingConsumerInvariant -Times 5 -Exactly
      }

      It 'blocks restore consumers when a verified staging folder is renamed and replaced' {
        $backupFolder = Join-Path $TestDrive 'replacement-source-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        $registryPath = Join-Path $backupFolder $script:UjBackupFileSystemProfile
        @'
Windows Registry Editor Version 5.00

[HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile]
"SystemResponsiveness"=-
"NetworkThrottlingIndex"=-

[HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Audio]
"Priority"=-
"BackgroundOnly"=-
"Clock Rate"=-
"SchedulingCategory"=-
"SFIOPriority"=-
'@ | Set-Content -LiteralPath $registryPath -Encoding Unicode
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{
            SystemProfile = $true
            AfdParameters = $false
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            PowerPlan = $false
          }
          ArtifactDigests = @{
            $script:UjBackupFileSystemProfile = (Get-FileHash -LiteralPath $registryPath -Algorithm SHA256).Hash
          }
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        $script:movedStagingPath = $null
        Mock -CommandName Assert-UjRestoreStagingConsumerInvariant {
          param($Session, [hashtable]$Manifest)

          $script:movedStagingPath = "$($Session.Path)-moved"
          try {
            Move-Item -LiteralPath $Session.Path -Destination $script:movedStagingPath -ErrorAction Stop
            [System.IO.Directory]::CreateDirectory([string]$Session.Path) | Out-Null
          } catch {
            throw "Staging replacement attempt could not preserve the verified session: $($_.Exception.Message)"
          }

          $check = Test-UjRestoreStagingInvariant -Session $Session -Manifest $Manifest
          if (-not $check.IsValid) { throw $check.Message }
        }
        Mock -CommandName Import-UjRegistryFile { throw 'must not import after staging replacement' }
        Mock -CommandName Restore-UjRegistryFromBackup { throw 'must not reach restore consumers' }
        Mock -CommandName Restore-UjQosFromBackup { throw 'must not reach restore consumers' }
        Mock -CommandName Restore-UjNicFromBackup { throw 'must not reach restore consumers' }
        Mock -CommandName Restore-UjRscFromBackup { throw 'must not reach restore consumers' }
        Mock -CommandName Restore-UjPowerPlanFromBackup { throw 'must not reach restore consumers' }

        try {
          $result = Restore-UjState -BackupFolder $backupFolder

          $result['Manifest'] | Should -Be 'Warn'
          $result['Registry'] | Should -Be 'Skipped'
          Assert-MockCalled -CommandName Import-UjRegistryFile -Times 0 -Exactly
          Assert-MockCalled -CommandName Restore-UjRegistryFromBackup -Times 0 -Exactly
        } finally {
          if (-not [string]::IsNullOrWhiteSpace([string]$script:movedStagingPath)) {
            Remove-Item -LiteralPath $script:movedStagingPath -Recurse -Force -ErrorAction SilentlyContinue
          }
        }
      }

      It 'revalidates staging immediately before registry import and aborts all remaining consumers' {
        $backupFolder = Join-Path $TestDrive 'pre-import-replacement-source-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        $registryPath = Join-Path $backupFolder $script:UjBackupFileSystemProfile
        @'
Windows Registry Editor Version 5.00

[HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile]
"SystemResponsiveness"=-
"NetworkThrottlingIndex"=-

[HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Audio]
"Priority"=-
"BackgroundOnly"=-
"Clock Rate"=-
"SchedulingCategory"=-
"SFIOPriority"=-
'@ | Set-Content -LiteralPath $registryPath -Encoding Unicode
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{
            SystemProfile = $true
            AfdParameters = $false
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            PowerPlan = $false
          }
          ArtifactDigests = @{
            $script:UjBackupFileSystemProfile = (Get-FileHash -LiteralPath $registryPath -Algorithm SHA256).Hash
          }
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        $script:consumerGateCallCount = 0
        $script:movedPreImportStagingPath = $null
        Mock -CommandName Assert-UjRestoreStagingConsumerInvariant {
          param($Session, [hashtable]$Manifest)

          $script:consumerGateCallCount++
          if ($script:consumerGateCallCount -lt 3) {
            Assert-UjRestoreStagingInvariant -Session $Session -Manifest $Manifest
            return
          }

          $message = $null
          $script:movedPreImportStagingPath = "$($Session.Path)-moved"
          try {
            Move-Item -LiteralPath $Session.Path -Destination $script:movedPreImportStagingPath -ErrorAction Stop
            [System.IO.Directory]::CreateDirectory([string]$Session.Path) | Out-Null
            $check = Test-UjRestoreStagingInvariant -Session $Session -Manifest $Manifest
            if (-not $check.IsValid) { $message = $check.Message }
          } catch {
            $message = "Staging replacement attempt could not preserve the verified session: $($_.Exception.Message)"
          }

          $exception = [System.InvalidOperationException]::new($message)
          $exception.Data['NetworkLantern.RestoreStagingInvariant'] = $true
          throw $exception
        }
        Mock -CommandName Import-UjRegistryFile { throw 'must not import after pre-import replacement' }
        Mock -CommandName Restore-UjQosFromBackup { throw 'must abort remaining consumers' }
        Mock -CommandName Restore-UjNicFromBackup { throw 'must abort remaining consumers' }
        Mock -CommandName Restore-UjRscFromBackup { throw 'must abort remaining consumers' }
        Mock -CommandName Restore-UjPowerPlanFromBackup { throw 'must abort remaining consumers' }

        try {
          $result = Restore-UjState -BackupFolder $backupFolder -Confirm:$false

          $result['Manifest'] | Should -Be 'Warn'
          $result['Registry'] | Should -Be 'Warn'
          $result['Qos'] | Should -Be 'Skipped'
          Assert-MockCalled -CommandName Assert-UjRestoreStagingConsumerInvariant -Times 3 -Exactly
          Assert-MockCalled -CommandName Import-UjRegistryFile -Times 0 -Exactly
          Assert-MockCalled -CommandName Restore-UjQosFromBackup -Times 0 -Exactly
        } finally {
          if (-not [string]::IsNullOrWhiteSpace([string]$script:movedPreImportStagingPath)) {
            Remove-Item -LiteralPath $script:movedPreImportStagingPath -Recurse -Force -ErrorAction SilentlyContinue
          }
        }
      }

      It 'rejects a digested registry backup that writes an HKLM Run value' {
        $backupFolder = Join-Path $TestDrive 'crafted-registry-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        $registryPath = Join-Path $backupFolder $script:UjBackupFileSystemProfile
        @'
Windows Registry Editor Version 5.00

[HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile]
"SystemResponsiveness"=-
"NetworkThrottlingIndex"=-

[HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Audio]
"Priority"=-
"BackgroundOnly"=-
"Clock Rate"=-
"SchedulingCategory"=-
"SFIOPriority"=-
'@ | Set-Content -LiteralPath $registryPath -Encoding Unicode
        $craftedRegistryPath = Join-Path $backupFolder $script:UjBackupFileAfdParameters
        @'
Windows Registry Editor Version 5.00

[HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Run]
"NetworkDiagnosticsSuiteTest"="cmd.exe /c exit"
'@ | Set-Content -LiteralPath $craftedRegistryPath -Encoding Unicode
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{
            SystemProfile = $true
            AfdParameters = $true
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            PowerPlan = $false
          }
          ArtifactDigests = @{
            $script:UjBackupFileSystemProfile = (Get-FileHash -LiteralPath $registryPath -Algorithm SHA256).Hash
            $script:UjBackupFileAfdParameters = (Get-FileHash -LiteralPath $craftedRegistryPath -Algorithm SHA256).Hash
          }
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        Mock -CommandName Import-UjRegistryFile { throw 'unapproved registry file reached import' }
        Mock -CommandName Restore-UjQosFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjNicFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjRscFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjPowerPlanFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }

        $result = Restore-UjState -BackupFolder $backupFolder

        $result['Registry'] | Should -Be 'Warn'
        Assert-MockCalled -CommandName Import-UjRegistryFile -Times 0 -Exactly
      }

      It 'restores a digested registry backup that contains only approved value state' {
        $backupFolder = Join-Path $TestDrive 'approved-registry-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        $registryPath = Join-Path $backupFolder $script:UjBackupFileSystemProfile
        @'
Windows Registry Editor Version 5.00

[HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile]
"SystemResponsiveness"=-
"NetworkThrottlingIndex"=dword:ffffffff

[HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Audio]
"Priority"=-
"BackgroundOnly"=-
"Clock Rate"=-
"SchedulingCategory"="High"
"SFIOPriority"=-
'@ | Set-Content -LiteralPath $registryPath -Encoding Unicode
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{
            SystemProfile = $true
            AfdParameters = $false
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            PowerPlan = $false
          }
          ArtifactDigests = @{
            $script:UjBackupFileSystemProfile = (Get-FileHash -LiteralPath $registryPath -Algorithm SHA256).Hash
          }
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        Mock -CommandName Import-UjRegistryFile { $true }
        Mock -CommandName Restore-UjQosFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjNicFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjRscFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }
        Mock -CommandName Restore-UjPowerPlanFromBackup { Get-UjRestoreComponentResult -Status 'Skipped' }

        $result = Restore-UjState -BackupFolder $backupFolder -Confirm:$false

        $result['Registry'] | Should -Be 'OK'
        Assert-MockCalled -CommandName Import-UjRegistryFile -Times 1 -Exactly
      }

      It 'rejects untrusted restore paths before artifact digests or restore work' {
        $backupFolder = Join-Path $TestDrive 'untrusted-path-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        $powerPlanPath = Join-Path $backupFolder $script:UjBackupFilePowerplan
        Set-Content -LiteralPath $powerPlanPath -Value $script:UjPowerPlanGuidBalanced -Encoding UTF8
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{
            SystemProfile = $false
            AfdParameters = $false
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            PowerPlan = $true
          }
          ArtifactDigests = @{
            $script:UjBackupFilePowerplan = (Get-FileHash -LiteralPath $powerPlanPath -Algorithm SHA256).Hash
          }
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

      It 'rejects a restore folder that is a symbolic link or reparse point' {
        $realFolder = Join-Path $TestDrive 'real-backup-folder'
        $linkedFolder = Join-Path $TestDrive 'linked-backup-folder'
        New-Item -ItemType Directory -Path $realFolder -Force | Out-Null
        $powerPlanPath = Join-Path $realFolder $script:UjBackupFilePowerplan
        Set-Content -LiteralPath $powerPlanPath -Value $script:UjPowerPlanGuidBalanced -Encoding UTF8
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{
            SystemProfile = $false
            AfdParameters = $false
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            PowerPlan = $true
          }
          ArtifactDigests = @{
            $script:UjBackupFilePowerplan = (Get-FileHash -LiteralPath $powerPlanPath -Algorithm SHA256).Hash
          }
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $realFolder 'backup_manifest.json') -Encoding UTF8

        try {
          New-Item -ItemType SymbolicLink -Path $linkedFolder -Target $realFolder -ErrorAction Stop | Out-Null
        } catch {
          Set-ItResult -Skipped -Because 'symbolic-link creation is unavailable on this test host'
          return
        }

        $result = Read-UjBackupManifest -BackupFolder $linkedFolder

        $result.Status | Should -Be 'Invalid'
        $result.Message | Should -Match 'symbolic link|reparse point'
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
          Components = @{
            SystemProfile = $false
            AfdParameters = $false
            QosPolicies = $false
            NicAdvanced = $false
            NicRsc = $false
            PowerPlan = $true
          }
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
        $script:UjNetshInvocations = [System.Collections.Generic.List[string]]::new()
        function netsh {
          $script:UjNetshInvocations.Add(($args -join ' ')) | Out-Null
          $global:LASTEXITCODE = 0
        }
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
        $script:UjNetshInvocations.Count | Should -Be 11
        $script:UjNetshInvocations | Should -Contain 'int tcp set global autotuninglevel=normal'
      }
    }
  }

  It 'CLI wrapper resolves default backup folder via module function' {
    $scriptPath = Join-Path -Path $PSScriptRoot -ChildPath '../../apps/windows-tuning/Invoke-NetworkPathTuning.ps1'
    $result = & $scriptPath -Action Backup -DryRun -PassThru

    $result | Should -Not -BeNullOrEmpty
    $result.BackupFolder | Should -Be (Get-NetworkLanternDefaultBackupFolder)
  }

  It 'CLI wrapper emits no structured result unless PassThru was requested' {
    $scriptPath = Join-Path -Path $PSScriptRoot -ChildPath '../../apps/windows-tuning/Invoke-NetworkPathTuning.ps1'

    $result = @(& $scriptPath -Action Backup -DryRun)

    $result.Count | Should -Be 0
  }

  It 'CLI wrapper exits nonzero for an unsuccessful result without PassThru' {
    $scriptPath = Join-Path -Path $PSScriptRoot -ChildPath '../../apps/windows-tuning/Invoke-NetworkPathTuning.ps1'

    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $scriptPath -Action Verify -UdpPorts 5201 -DryRun 2>&1

    $LASTEXITCODE | Should -Be 1
    ($output | Out-String) | Should -Not -Match '^\s*Success\s*:'
  }

  It 'CLI wrapper exits nonzero when a restore manifest has no components' {
    $scriptPath = Join-Path -Path $PSScriptRoot -ChildPath '../../apps/windows-tuning/Invoke-NetworkPathTuning.ps1'
    $backupFolder = Join-Path $TestDrive 'cli-empty-components-backup'
    New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
    @{
      SchemaVersion = 1
      ToolName = 'network-diagnostics-suite'
      Timestamp = '2026-01-01T00:00:00Z'
      Components = @{}
      ArtifactDigests = @{}
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $scriptPath -Action Restore -BackupFolder $backupFolder -DryRun 2>&1

    $LASTEXITCODE | Should -Be 1
    ($output | Out-String) | Should -Not -Match '^\s*Success\s*:'
  }

  It 'CLI wrapper exits nonzero for every invalid restore-manifest JSON shape' {
    $scriptPath = Join-Path -Path $PSScriptRoot -ChildPath '../../apps/windows-tuning/Invoke-NetworkPathTuning.ps1'
    $invalidPayloads = [ordered]@{
      NullRoot = 'null'
      ArrayRoot = '[]'
      ScalarRoot = '"manifest"'
      NonnumericSchema = '{"SchemaVersion":"one","Components":{}}'
    }

    foreach ($caseName in $invalidPayloads.Keys) {
      $backupFolder = Join-Path $TestDrive "cli-invalid-shape-$caseName"
      New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
      Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Value $invalidPayloads[$caseName] -Encoding UTF8

      $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $scriptPath -Action Restore -BackupFolder $backupFolder -DryRun 2>&1

      $LASTEXITCODE | Should -Be 1 -Because "$caseName must fail closed"
      ($output | Out-String) | Should -Not -Match '^\s*Success\s*:'
    }
  }

  It 'GUI entrypoint is informational instead of failing' {
    $scriptPath = Join-Path -Path $PSScriptRoot -ChildPath '../../apps/windows-tuning/Invoke-NetworkPathTuning-GUI.ps1'
    $output = & pwsh -NoLogo -NoProfile -File $scriptPath 2>&1

    $LASTEXITCODE | Should -Be 0
    $message = $output | Out-String
    $message | Should -Match 'CLI-first'
    $message | Should -Match 'compatibility entrypoint'
    $message | Should -Match 'does not launch a GUI'
  }

  Context 'Backup action' {
    InModuleScope 'NetworkLantern.WindowsTuning' {
      It 'Backup action with PassThru returns a structured result' {
        Mock -CommandName Backup-UjState {
          [pscustomobject]@{ Status = 'OK'; Message = 'Backup complete.' }
        }
        Mock -CommandName Resolve-UjRestoreStatus { 'OK' }

        $result = Invoke-NetworkPathTuning -Action Backup -DryRun -PassThru

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

      It 'writes an explicit restorable artifact when no managed QoS policies exist' {
        $backupFolder = Join-Path $TestDrive 'empty-qos-backup'
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

        Mock -CommandName Export-UjRegistryKey { $false }
        Mock -CommandName Get-UjManagedQosPolicy { @() }
        Mock -CommandName Get-UjPhysicalUpAdapter { @() }
        Mock -CommandName Get-NetAdapterRsc { @() }

        $null = Backup-UjState -BackupFolder $backupFolder -Confirm:$false
        $qosPath = Join-Path $backupFolder $script:UjBackupFileQosOurs
        $manifest = Get-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Raw | ConvertFrom-Json

        Test-Path -LiteralPath $qosPath -PathType Leaf | Should -BeTrue
        $restoredQos = Import-Clixml -LiteralPath $qosPath
        @($restoredQos).Count | Should -Be 0
        $manifest.Components.QosPolicies | Should -BeTrue
      }

      It 'does not mark missing NIC RSC or power-plan artifacts complete' {
        $backupFolder = Join-Path $TestDrive 'missing-optional-artifacts-backup'
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

        Mock -CommandName Export-UjRegistryKey { $false }
        Mock -CommandName Get-UjManagedQosPolicy { @() }
        Mock -CommandName Get-UjPhysicalUpAdapter { @() }
        Mock -CommandName Get-NetAdapterRsc { @() }

        $result = Backup-UjState -BackupFolder $backupFolder -Confirm:$false
        $manifest = Get-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Raw | ConvertFrom-Json

        $result.Status | Should -Be 'Warn'
        $manifest.Components.NicAdvanced | Should -BeFalse
        $manifest.Components.NicRsc | Should -BeFalse
        $manifest.Components.PowerPlan | Should -BeFalse
      }
    }
  }

  Context 'Restore action' {
    InModuleScope 'NetworkLantern.WindowsTuning' {
    It 'Restore action with a valid manifest succeeds when all components report OK' {
        Mock -CommandName Restore-UjState {
          @{ Registry = 'OK'; QosPolicies = 'OK'; Manifest = 'OK' }
        }

        $result = Invoke-NetworkPathTuning -Action Restore -DryRun -PassThru

        $result | Should -Not -BeNullOrEmpty
        $result.Action | Should -Be 'Restore'
        $result.Success | Should -BeTrue
        $result.Components['Registry'] | Should -Be 'OK'
        $result.Components['QosPolicies'] | Should -Be 'OK'
      }

      It 'Restore action reports failure when a dry-run manifest has no components' {
        $backupFolder = Join-Path $TestDrive 'public-empty-components-backup'
        New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
        @{
          SchemaVersion = $script:UjBackupSchemaVersion
          ToolName = 'network-diagnostics-suite'
          Timestamp = '2026-01-01T00:00:00Z'
          Components = @{}
          ArtifactDigests = @{}
        } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Encoding UTF8

        $result = Invoke-NetworkPathTuning -Action Restore -BackupFolder $backupFolder -DryRun -PassThru

        $result.Success | Should -BeFalse
        $result.Components['Manifest'] | Should -Be 'Warn'
      }

      It 'Restore action reports failure for every invalid manifest JSON shape' {
        $invalidPayloads = [ordered]@{
          NullRoot = 'null'
          ArrayRoot = '[]'
          ScalarRoot = '"manifest"'
          NonnumericSchema = '{"SchemaVersion":"one","Components":{}}'
        }

        foreach ($caseName in $invalidPayloads.Keys) {
          $backupFolder = Join-Path $TestDrive "public-invalid-shape-$caseName"
          New-Item -ItemType Directory -Path $backupFolder -Force | Out-Null
          Set-Content -LiteralPath (Join-Path $backupFolder 'backup_manifest.json') -Value $invalidPayloads[$caseName] -Encoding UTF8

          $result = Invoke-NetworkPathTuning -Action Restore -BackupFolder $backupFolder -DryRun -PassThru

          $result.Success | Should -BeFalse -Because "$caseName must fail closed"
          $result.Components['Manifest'] | Should -Be 'Warn'
        }
      }
    }

    InModuleScope 'NetworkLantern.WindowsTuning' {
      It 'falls back to the legacy default backup when the new default has no manifest' {
        $previousDefault = $script:UjDefaultBackupFolder
        $previousLegacyDefault = $script:UjLegacyDefaultBackupFolder
        try {
          $script:UjDefaultBackupFolder = Join-Path $TestDrive 'NetworkLantern'
          $script:UjLegacyDefaultBackupFolder = Join-Path $TestDrive 'NetworkDiagnosticsSuite'
          New-Item -ItemType Directory -Path $script:UjLegacyDefaultBackupFolder -Force | Out-Null
          Set-Content -LiteralPath (Join-Path $script:UjLegacyDefaultBackupFolder 'backup_manifest.json') -Value '{}' -Encoding UTF8
          Mock -CommandName Restore-UjState { [ordered]@{ Manifest = 'OK' } }

          $result = Invoke-NetworkPathTuning -Action Restore -DryRun -PassThru

          $result.Success | Should -BeTrue
          $result.BackupFolder | Should -Be $script:UjLegacyDefaultBackupFolder
          Should -Invoke -CommandName Restore-UjState -Times 1 -Exactly -ParameterFilter {
            $BackupFolder -eq $script:UjLegacyDefaultBackupFolder
          }
        } finally {
          $script:UjDefaultBackupFolder = $previousDefault
          $script:UjLegacyDefaultBackupFolder = $previousLegacyDefault
        }
      }
    }
  }

  Context 'IncludeAppPolicies flag' {
    InModuleScope 'NetworkLantern.WindowsTuning' {
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

      { Invoke-NetworkPathTuning -Action Apply } | Should -Throw '*Windows*'
    }
  }

  Context 'Backup manifest metadata' {
    InModuleScope 'NetworkLantern.WindowsTuning' {
      It 'Get-UjBackupManifestMetadata returns required enrichment fields' {
        $metadata = Get-UjBackupManifestMetadata

        $metadata | Should -Not -BeNullOrEmpty
        $metadata['SchemaVersion'] | Should -BeGreaterThan 0
        $metadata['ToolName'] | Should -Be 'network-lantern'
        $metadata['MachineName'] | Should -Not -BeNullOrEmpty
        $metadata['Platform'] | Should -Not -BeNullOrEmpty
        $metadata['OsVersion'] | Should -Not -BeNullOrEmpty
      }
    }
  }
}
