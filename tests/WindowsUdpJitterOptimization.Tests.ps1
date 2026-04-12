Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ManifestPath = Join-Path -Path $PSScriptRoot -ChildPath '../WindowsUdpJitterOptimization/WindowsUdpJitterOptimization.psd1'
Import-Module -Name $ManifestPath -Force

Describe 'WindowsUdpJitterOptimization repo' {
  It 'exposes Invoke-UdpJitterOptimization' {
    (Get-Command -Name Invoke-UdpJitterOptimization -ErrorAction Stop).CommandType | Should -Be 'Function'
  }

  It 'exposes Get-UjDefaultBackupFolder' {
    (Get-Command -Name Get-UjDefaultBackupFolder -ErrorAction Stop).CommandType | Should -Be 'Function'
  }

  It 'Get-UjDefaultBackupFolder returns a path ending with UDPTune' {
    $path = Get-UjDefaultBackupFolder
    $path | Should -Not -BeNullOrEmpty
    $path | Should -Match 'UDPTune$'
  }

  It 'exposes Test-UjIsAdministrator' {
    (Get-Command -Name Test-UjIsAdministrator -ErrorAction Stop).CommandType | Should -Be 'Function'
  }

  It 'Test-UjIsAdministrator returns bool' {
    $result = Test-UjIsAdministrator
    $result | Should -BeIn @($true, $false)
  }

  It 'does not use Invoke-Expression' {
    $hits = Get-ChildItem -Path (Join-Path -Path $PSScriptRoot -ChildPath '..') -Recurse -File -Include '*.ps1', '*.psm1' |
      Where-Object { $_.FullName -notmatch '[\\\\/]tests[\\\\/]' } |
      Select-String -Pattern 'Invoke-Expression' -SimpleMatch -ErrorAction SilentlyContinue
    $hits | Should -BeNullOrEmpty
  }

  Context 'DryRun safety' {
    InModuleScope WindowsUdpJitterOptimization {
      It 'skips MMCSS audio task tuning on DryRun' {
        Mock -CommandName Set-UjRegistryValue
        Mock -CommandName New-Item

        Set-UjMmcssAudioTaskTuning -DryRun

        Assert-MockCalled -CommandName Set-UjRegistryValue -Times 0
        Assert-MockCalled -CommandName New-Item -Times 0
      }

      It 'skips SystemResponsiveness on DryRun' {
        Mock -CommandName Set-UjRegistryValue

        Set-UjSystemResponsiveness -Preset 1 -DryRun

        Assert-MockCalled -CommandName Set-UjRegistryValue -Times 0
      }

      It 'skips audio service changes on DryRun' {
        { Start-UjAudioService -DryRun } | Should -Not -Throw
      }

      It 'skips local QoS registry changes on DryRun' {
        Mock -CommandName Set-UjRegistryValue
        Mock -CommandName New-Item

        Enable-UjLocalQosMarking -DryRun

        Assert-MockCalled -CommandName Set-UjRegistryValue -Times 0
        Assert-MockCalled -CommandName New-Item -Times 0
      }

      It 'skips GameDVR changes on DryRun' {
        Mock -CommandName New-ItemProperty

        Set-UjGameDvrState -State Disabled -DryRun

        Assert-MockCalled -CommandName New-ItemProperty -Times 0
      }

      It 'Backup with DryRun creates no files' {
        Mock -CommandName New-UjDirectory
        Mock -CommandName Export-UjRegistryKey
        Mock -CommandName Out-File

        Backup-UjState -BackupFolder (Join-Path -Path $TestDrive -ChildPath 'Backup') -DryRun

        Assert-MockCalled -CommandName New-UjDirectory -Times 0
        Assert-MockCalled -CommandName Export-UjRegistryKey -Times 0
        Assert-MockCalled -CommandName Out-File -Times 0
      }
    }
  }

  Context 'Managed QoS ownership' {
    InModuleScope WindowsUdpJitterOptimization {
      It 'Get-UjManagedQosPolicy filters to module-owned prefixes only' {
        function Get-NetQosPolicy {
          @(
            [pscustomobject]@{ Name = 'QoS_UDP_TS_9987' },
            [pscustomobject]@{ Name = 'QoS_UDP_CS2_27015' },
            [pscustomobject]@{ Name = 'QoS_APP_1' },
            [pscustomobject]@{ Name = 'QoS_CUSTOM_OTHER' },
            [pscustomobject]@{ Name = 'UnrelatedPolicy' }
          )
        }

        $names = @(Get-UjManagedQosPolicy | Select-Object -ExpandProperty Name)

        $names | Should -Contain 'QoS_UDP_TS_9987'
        $names | Should -Contain 'QoS_UDP_CS2_27015'
        $names | Should -Contain 'QoS_APP_1'
        $names | Should -Not -Contain 'QoS_CUSTOM_OTHER'
        $names | Should -Not -Contain 'UnrelatedPolicy'
      }
    }
  }

  Context 'Bundle A features' {
    InModuleScope WindowsUdpJitterOptimization {
      It 'unsafe backup folder detector flags sensitive system directories' {
        (Test-UjUnsafeBackupFolder -Path 'C:\Windows\System32\UDPTune') | Should -BeTrue
        (Test-UjUnsafeBackupFolder -Path 'C:\Program Files\UDPTune') | Should -BeTrue
        (Test-UjUnsafeBackupFolder -Path 'C:\Program Files (x86)\UDPTune') | Should -BeTrue
      }

      It 'unsafe backup folder detector allows non-sensitive paths' {
        (Test-UjUnsafeBackupFolder -Path 'C:\ProgramData\UDPTune') | Should -BeFalse
        (Test-UjUnsafeBackupFolder -Path (Join-Path -Path $TestDrive -ChildPath 'SafeBackup')) | Should -BeFalse
      }

      It 'Apply DryRun with PassThru returns a structured result' {
        Mock -CommandName Backup-UjState
        Mock -CommandName Set-UjMmcssAudioTaskTuning
        Mock -CommandName Start-UjAudioService
        Mock -CommandName Set-UjSystemResponsiveness
        Mock -CommandName Enable-UjLocalQosMarking
        Mock -CommandName New-UjDscpPolicyByPort
        Mock -CommandName New-UjDscpPolicyByApp
        Mock -CommandName Set-UjNicConfiguration
        Mock -CommandName Set-UjAfdFastSendDatagramThreshold
        Mock -CommandName Set-UjNetworkThrottlingIndex
        Mock -CommandName Set-UjUroState
        Mock -CommandName Set-UjPowerPlan
        Mock -CommandName Set-UjGameDvrState
        Mock -CommandName Show-UjSummary

        $result = Invoke-UdpJitterOptimization -Action Apply -DryRun -SkipAdminCheck -PassThru -Confirm:$false

        $result | Should -Not -BeNullOrEmpty
        $result.Action | Should -Be 'Apply'
        $result.Preset | Should -Be 1
        $result.IncludeExperimental | Should -BeFalse
        $result.DryRun | Should -BeTrue
        $result.Success | Should -BeTrue
        $result.BackupFolder | Should -Match 'UDPTune$'
        $result.Timestamp | Should -Not -BeNullOrEmpty
        $result.Components | Should -Not -BeNullOrEmpty
      }

      It 'blocks unsafe backup folder by default' {
        {
          Invoke-UdpJitterOptimization -Action Backup -BackupFolder 'C:\Windows\System32\UDPTune' -SkipAdminCheck -DryRun -Confirm:$false
        } | Should -Throw '*unsafe*'
      }

      It 'allows unsafe backup folder with override switch' {
        Mock -CommandName Backup-UjState

        {
          Invoke-UdpJitterOptimization -Action Backup -BackupFolder 'C:\Windows\System32\UDPTune' -AllowUnsafeBackupFolder -SkipAdminCheck -DryRun -Confirm:$false
        } | Should -Not -Throw
      }

      It 'Restore-UjState returns component status map' {
        Mock -CommandName Restore-UjRegistryFromBackup { [pscustomobject]@{ Status = 'OK'; Message = '' } }
        Mock -CommandName Restore-UjQosFromBackup { @{ Status = 'OK' } }
        Mock -CommandName Restore-UjNicFromBackup { @{ Status = 'Warn' } }
        Mock -CommandName Restore-UjRscFromBackup { @{ Status = 'Skipped' } }
        Mock -CommandName Restore-UjPowerPlanFromBackup { @{ Status = 'OK' } }

        $result = Restore-UjState -BackupFolder (Join-Path -Path $TestDrive -ChildPath 'Backup') -Confirm:$false

        $result | Should -Not -BeNullOrEmpty
        $result.Registry | Should -Be 'OK'
        $result.Qos | Should -Be 'OK'
        $result.NicAdvanced | Should -Be 'Warn'
        $result.Rsc | Should -Be 'Skipped'
        $result.PowerPlan | Should -Be 'OK'
      }
    }

    It 'CLI wrapper resolves default backup folder via module and supports PassThru' {
      $scriptPath = Join-Path -Path $PSScriptRoot -ChildPath '../optimize-udp-jitter.ps1'
      $result = & $scriptPath -Action Backup -DryRun -SkipAdminCheck -PassThru -Confirm:$false

      $result | Should -Not -BeNullOrEmpty
      $result.BackupFolder | Should -Be (Get-UjDefaultBackupFolder)
    }
  }

  Context 'GUID helper' {
    InModuleScope WindowsUdpJitterOptimization {
      It 'Get-UjGuidFromText returns braced GUID when input already contains braces' {
        $result = Get-UjGuidFromText -Text '{8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c}'
        $result | Should -Be '{8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c}'
      }

      It 'Get-UjGuidFromText wraps bare GUID in braces' {
        $result = Get-UjGuidFromText -Text 'Power Scheme GUID: 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c  (High performance)'
        $result | Should -Be '{8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c}'
      }

      It 'Get-UjGuidFromText returns null when no GUID is present' {
        $result = Get-UjGuidFromText -Text 'no guid here'
        $result | Should -BeNullOrEmpty
      }
    }
  }

  Context 'Power plan constants' {
    InModuleScope WindowsUdpJitterOptimization {
      It 'power plan GUID constants are defined and non-empty' {
        $script:UjPowerPlanGuidBalanced        | Should -Not -BeNullOrEmpty
        $script:UjPowerPlanGuidHighPerformance | Should -Not -BeNullOrEmpty
        $script:UjPowerPlanGuidUltimate        | Should -Not -BeNullOrEmpty
      }

      It 'power plan GUID constants match GUID format' {
        $guidPattern = '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$'
        $script:UjPowerPlanGuidBalanced        | Should -Match $guidPattern
        $script:UjPowerPlanGuidHighPerformance | Should -Match $guidPattern
        $script:UjPowerPlanGuidUltimate        | Should -Match $guidPattern
      }
    }
  }

  Context 'NIC keyword tier classification' {
    InModuleScope WindowsUdpJitterOptimization {
      It 'tier arrays have no overlap' {
        $all = @($script:UjNicKeywordsTier1 + $script:UjNicKeywordsTier2 + $script:UjNicKeywordsTier3 + $script:UjNicKeywordsExperimental)
        $unique = $all | Select-Object -Unique
        $all.Count | Should -Be $unique.Count
      }

      It 'UjNicResetKeywords is the union of all tier arrays' {
        $expected = @($script:UjNicKeywordsTier1 + $script:UjNicKeywordsTier2 + $script:UjNicKeywordsTier3 + $script:UjNicKeywordsExperimental)
        $script:UjNicResetKeywords.Count | Should -Be $expected.Count
        foreach ($kw in $expected) {
          $script:UjNicResetKeywords | Should -Contain $kw
        }
      }

      It 'reverse map covers all keywords in all tier arrays' {
        $allKeywords = @($script:UjNicKeywordsTier1 + $script:UjNicKeywordsTier2 + $script:UjNicKeywordsTier3 + $script:UjNicKeywordsExperimental)
        foreach ($kw in $allKeywords) {
          $script:UjNicKeywordReverseMap.ContainsKey($kw) | Should -BeTrue -Because "keyword '$kw' should have a reverse map entry"
        }
      }
    }
  }

  Context 'Port overlap validation' {
    InModuleScope WindowsUdpJitterOptimization {
      It 'emits a warning when TeamSpeak port falls within CS2 port range' {
        Mock -CommandName Backup-UjState
        Mock -CommandName Set-UjMmcssAudioTaskTuning
        Mock -CommandName Start-UjAudioService
        Mock -CommandName Set-UjSystemResponsiveness
        Mock -CommandName Enable-UjLocalQosMarking
        Mock -CommandName New-UjDscpPolicyByPort
        Mock -CommandName Set-UjNicConfiguration
        Mock -CommandName Set-UjAfdFastSendDatagramThreshold
        Mock -CommandName Set-UjNetworkThrottlingIndex
        Mock -CommandName Show-UjSummary

        $capturedWarnings = $null
        Invoke-UdpJitterOptimization -Action Apply -SkipAdminCheck -DryRun -Confirm:$false `
          -TeamSpeakPort 27015 -CS2PortStart 27015 -CS2PortEnd 27036 `
          -WarningVariable capturedWarnings

        $capturedWarnings | Should -Not -BeNullOrEmpty
        ($capturedWarnings | Where-Object { $_ -match 'TeamSpeak port' }) | Should -Not -BeNullOrEmpty
      }
    }
  }
}
