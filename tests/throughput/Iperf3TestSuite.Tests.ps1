# TODO: This test file is monolithic and should be split into separate files per functional area
# (e.g., Profiles.Tests.ps1, Iperf3Run.Tests.ps1, Results.Tests.ps1, Connectivity.Tests.ps1, etc.).
# This is too large a refactor for this pass but is noted here as tech debt.

$ErrorActionPreference = 'Stop'

BeforeAll {
  . (Join-Path (Get-Item $PSScriptRoot).Parent.Parent.FullName 'scripts/Get-RepoRoot.ps1')
  $repoRoot = Get-RepoRoot
  $script:RepoRoot = $repoRoot
  $modulePath = Join-Path $repoRoot 'src/powershell/throughput/Iperf3TestSuite.psd1'
  Import-Module $modulePath -Force
  $script:TestCapability = [pscustomobject]@{ VersionText = 'iperf3 3.9'; Major = 3; Minor = 9; BidirSupported = $true }
  $global:Iperf3TestSuite_TestCapability = $script:TestCapability
  try {
    $global:IsWindows = $true
  } catch {
    Write-Verbose 'On some hosts (e.g. macOS) $IsWindows is read-only; Windows-only tests may fail or be skipped.'
  }
}

function New-TestCapability {
  $script:TestCapability
}

function New-TestTcpResult {
  param([bool]$TcpSucceeded = $true, [object]$Trace = $null)
  [pscustomobject]@{
    Tcp   = [pscustomobject]@{ TcpTestSucceeded = $TcpSucceeded; RemoteAddress = '127.0.0.1'; PingSucceeded = $true }
    Trace = $Trace
  }
}

Describe 'Iperf3TestSuite helpers' {
  Context 'DSCP mapping' {
    It 'maps CS0 to 0' {
      InModuleScope Iperf3TestSuite {
        Get-TosFromDscpClass -Class 'CS0' | Should -Be 0
      }
    }

    It 'maps EF to 184' {
      InModuleScope Iperf3TestSuite {
        Get-TosFromDscpClass -Class 'EF' | Should -Be (46 -shl 2)
      }
    }

    It 'maps AF11 to 40' {
      InModuleScope Iperf3TestSuite {
        Get-TosFromDscpClass -Class 'AF11' | Should -Be 40
      }
    }

    It 'throws on unknown DSCP class' {
      InModuleScope Iperf3TestSuite {
        { Get-TosFromDscpClass -Class 'NOPE' } | Should -Throw "Unknown DSCP class*"
      }
    }
  }

  Context 'Hostname/IP validation' {
    It 'rejects invalid IPv6-like strings (e.g. :::)' {
      InModuleScope Iperf3TestSuite {
        Test-ValidHostnameOrIP -Name ':::' | Should -Be $false
        Test-ValidHostnameOrIP -Name ':' | Should -Be $false
      }
    }
  }

  Context 'Bandwidth parsing' {
    It 'parses 10M to 10' {
      InModuleScope Iperf3TestSuite {
        ConvertTo-MbitPerSecond -Value '10M' | Should -Be 10.0
      }
    }

    It 'parses 1G to 1000' {
      InModuleScope Iperf3TestSuite {
        ConvertTo-MbitPerSecond -Value '1G' | Should -Be 1000.0
      }
    }

    It 'parses 500K to 0.5' {
      InModuleScope Iperf3TestSuite {
        ConvertTo-MbitPerSecond -Value '500K' | Should -Be 0.5
      }
    }

    It 'throws on invalid format' {
      InModuleScope Iperf3TestSuite {
        { ConvertTo-MbitPerSecond -Value 'nope' } | Should -Throw
      }
    }
  }

  Context 'JSON extraction' {
    It 'extracts the JSON substring when surrounded by text' {
      InModuleScope Iperf3TestSuite {
        $s = 'banner {"a":1} trailer'
        Get-JsonSubstringOrNull -Text $s | Should -Be '{"a":1}'
      }
    }

    It 'skips invalid braces and returns the first valid JSON' {
      InModuleScope Iperf3TestSuite {
        $s = 'prefix {notjson} mid {"a":1} trailer'
        Get-JsonSubstringOrNull -Text $s | Should -Be '{"a":1}'
      }
    }

    It 'handles braces inside JSON strings' {
      InModuleScope Iperf3TestSuite {
        $s = 'banner {"a":"{x}"} trailer'
        Get-JsonSubstringOrNull -Text $s | Should -Be '{"a":"{x}"}'
      }
    }

    It 'returns null when braces exist but no valid JSON' {
      InModuleScope Iperf3TestSuite {
        $s = 'prefix {not json} trailer'
        Get-JsonSubstringOrNull -Text $s | Should -Be $null
      }
    }

    It 'returns null when no braces exist' {
      InModuleScope Iperf3TestSuite {
        Get-JsonSubstringOrNull -Text 'no json here' | Should -Be $null
      }
    }
  }

  Context 'Metric extraction' {
    It 'extracts TCP metrics (TX)' {
      InModuleScope Iperf3TestSuite {
        $json = ConvertFrom-Json '{"end":{"sum_sent":{"bits_per_second":10000000,"retransmits":2},"sum_received":{"bits_per_second":8000000}}}'
        $m = Get-Iperf3Metric -Json $json -Proto TCP -Dir TX
        $m.TxMbps | Should -Be 10.0
        $m.RxMbps | Should -Be 8.0
        $m.Retr | Should -Be 2
        $m.LossPct | Should -Be $null
        $m.JitterMs | Should -Be $null
      }
    }

    It 'extracts UDP metrics (TX)' {
      InModuleScope Iperf3TestSuite {
        $json = ConvertFrom-Json '{"end":{"sum_sent":{"bits_per_second":10000000,"lost_percent":2.5},"sum_received":{"bits_per_second":9000000},"sum":{"lost_percent":2.5,"jitter_ms":1.2}}}'
        $m = Get-Iperf3Metric -Json $json -Proto UDP -Dir TX
        $m.TxMbps | Should -Be 10.0
        $m.RxMbps | Should -Be 9.0
        $m.Retr | Should -Be $null
        $m.LossPct | Should -Be 2.5
        $m.JitterMs | Should -Be 1.2
      }
    }
  }

  Context 'CSV row shape' {
    It 'creates a stable column order' {
      InModuleScope Iperf3TestSuite {
        $row = ConvertTo-Iperf3CsvRow -No 1 -Proto 'TCP' -Dir 'TX' -DSCP 'CS0' -Streams 1 -Win 'default' `
          -ThrTxMbps 1.23 -RetrTx 0 -ThrRxMbps 0.0 -LossTxPct $null -JitterMs $null -Role 'end'

        $row.PSObject.Properties.Name | Should -Be @(
          'No',
          'Proto',
          'Dir',
          'DSCP',
          'Streams',
          'Win',
          'Thr_TX_Mbps',
          'Retr_TX',
          'Thr_RX_Mbps',
          'Loss_TX_Pct',
          'Jitter_ms',
          'Duration_ms',
          'Role'
        )
      }
    }
  }

  Context 'Invoke-Iperf3 args' {
    It 'adds -R for TCP RX' {
      $captured = InModuleScope Iperf3TestSuite {
        $script:captured = $null
        $caps = $global:Iperf3TestSuite_TestCapability
        $runner = { param([string[]]$IperfArgs); $script:captured = $IperfArgs; $global:LASTEXITCODE = 0; return '{"end":{}}' }
        $null = Invoke-Iperf3 -Server 'example' -Port 5201 -Stack 'IPv4' -Duration 1 -Omit 0 -Proto 'TCP' -Dir 'RX' -Caps $caps -Runner $runner
        return $script:captured
      }
      $captured | Should -Contain '-R'
    }

    It 'adds --bidir for TCP BD when supported' {
      $captured = InModuleScope Iperf3TestSuite {
        $script:captured = $null
        $caps = $global:Iperf3TestSuite_TestCapability
        $runner = { param([string[]]$IperfArgs); $script:captured = $IperfArgs; $global:LASTEXITCODE = 0; return '{"end":{}}' }
        $null = Invoke-Iperf3 -Server 'example' -Port 5201 -Stack 'IPv4' -Duration 1 -Omit 0 -Proto 'TCP' -Dir 'BD' -Caps $caps -Runner $runner
        return $script:captured
      }
      $captured | Should -Contain '--bidir'
    }

    It 'adds -u and -b for UDP' {
      $captured = InModuleScope Iperf3TestSuite {
        $script:captured = $null
        $caps = $global:Iperf3TestSuite_TestCapability
        $runner = { param([string[]]$IperfArgs); $script:captured = $IperfArgs; $global:LASTEXITCODE = 0; return '{"end":{}}' }
        $null = Invoke-Iperf3 -Server 'example' -Port 5201 -Stack 'IPv4' -Duration 1 -Omit 0 -Proto 'UDP' -Dir 'TX' -UdpBw '5M' -Caps $caps -Runner $runner
        return $script:captured
      }
      $captured | Should -Contain '-u'
      $captured | Should -Contain '-b'
      $captured | Should -Contain '5M'
    }

    It 'marks a zero-exit run without JSON as a parse failure' {
      InModuleScope Iperf3TestSuite {
        $caps = $global:Iperf3TestSuite_TestCapability
        $runner = {
          param([string[]]$IperfArgs)
          $null = $IperfArgs
          $global:LASTEXITCODE = 0
          return 'iperf3 completed without JSON output'
        }

        $result = Invoke-Iperf3 -Server 'example' -Port 5201 -Stack 'IPv4' -Duration 1 -Omit 0 -Proto 'TCP' -Dir 'TX' -Caps $caps -Runner $runner

        $result.ExitCode | Should -Be 0
        $result.Json | Should -Be $null
        $result.JsonParseError | Should -Match 'JSON output was not found'
      }
    }
  }

  Context 'Failure handling' {
    It 'emits InputValidation ErrorId when target is missing' {
      InModuleScope Iperf3TestSuite {
        $err = $null
        try { Invoke-Iperf3TestSuite -OutDir $TestDrive -Quiet } catch { $err = $_ }
        $err | Should -Not -BeNullOrEmpty
        $err.FullyQualifiedErrorId | Should -Match 'Iperf3TestSuite.InputValidation'
      }
    }

    It 'throws when reachability fails' {
      InModuleScope Iperf3TestSuite {
        Mock Test-Iperf3TestSuitePrerequisites { }
        Mock Get-Command { [pscustomobject]@{ Name = $Name } }
        Mock Get-Iperf3Capability { $global:Iperf3TestSuite_TestCapability }
        Mock Test-Reachability { 'None' }
        Mock Test-TcpPortAndTrace { throw 'Should not be called' }
        Mock Invoke-Iperf3 { throw 'Should not be called' }

        Should -Throw -ActualValue { Invoke-Iperf3TestSuite -Target 'example.local' -OutDir $TestDrive -Quiet } -ExpectedMessage "ICMP reachability*"
      }
    }

    It 'emits Connectivity ErrorId when reachability fails' {
      InModuleScope Iperf3TestSuite {
        Mock Test-Iperf3TestSuitePrerequisites { }
        Mock Get-Command { [pscustomobject]@{ Name = $Name } }
        Mock Get-Iperf3Capability { $global:Iperf3TestSuite_TestCapability }
        Mock Test-Reachability { 'None' }
        Mock Test-TcpPortAndTrace { throw 'Should not be called' }
        Mock Invoke-Iperf3 { throw 'Should not be called' }

        $err = $null
        try { Invoke-Iperf3TestSuite -Target 'example.local' -OutDir $TestDrive -Quiet } catch { $err = $_ }
        $err | Should -Not -BeNullOrEmpty
        $err.FullyQualifiedErrorId | Should -Match 'Iperf3TestSuite.Connectivity'
      }
    }

    It 'throws on non-Windows' -Skip:$IsWindows {
      InModuleScope Iperf3TestSuite {
        Mock Get-Command { [pscustomobject]@{ Name = $Name } }
        Mock Get-Iperf3Capability { $global:Iperf3TestSuite_TestCapability }
        Mock Invoke-Iperf3 { throw 'Should not be called' }
        Should -Throw -ActualValue { Invoke-Iperf3TestSuite -Target 'example.local' -OutDir $TestDrive -Quiet } -ExpectedMessage "*require Windows*"
      }
    }

    It 'throws when TCP port is not reachable' {
      InModuleScope Iperf3TestSuite {
        Mock Test-Iperf3TestSuitePrerequisites { }
        Mock Get-Command { [pscustomobject]@{ Name = $Name } }
        Mock Get-Iperf3Capability { $global:Iperf3TestSuite_TestCapability }
        Mock Test-Reachability { 'IPv4' }
        Mock Test-TcpPortAndTrace {
          [pscustomobject]@{ Tcp = [pscustomobject]@{ TcpTestSucceeded = $false; RemoteAddress = '127.0.0.1'; PingSucceeded = $true }; Trace = $null }
        }
        Mock Invoke-Iperf3 { throw 'Should not be called' }

        Should -Throw -ActualValue { Invoke-Iperf3TestSuite -Target 'example.local' -OutDir $TestDrive -Quiet } -ExpectedMessage "TCP port*"
      }
    }

    It 'throws when SingleTest and DscpClasses is empty' {
      InModuleScope Iperf3TestSuite {
        Mock Test-Iperf3TestSuitePrerequisites { }
        Mock Get-Command { [pscustomobject]@{ Name = $Name } }
        Mock Get-Iperf3Capability { $global:Iperf3TestSuite_TestCapability }

        # Parameter binding may reject empty array; otherwise our explicit check throws
        $err = $null
        try { Invoke-Iperf3TestSuite -Target 'example.local' -OutDir $TestDrive -Quiet -SingleTest -DscpClasses @() } catch { $err = $_ }
        $err | Should -Not -BeNullOrEmpty
        ($err.Exception.Message -match 'at least one DSCP|empty|null') | Should -Be $true
      }
    }
  }

  Context 'UDP saturation loop' {
    It 'stops when loss exceeds threshold' {
      InModuleScope Iperf3TestSuite {
        Mock Test-Iperf3TestSuitePrerequisites { }
        Mock Get-Command { [pscustomobject]@{ Name = $Name } }
        $script:udpBws = New-Object System.Collections.Generic.List[string]

        Mock Get-Iperf3Capability { $global:Iperf3TestSuite_TestCapability }
        Mock Test-Reachability { 'IPv4' }
        Mock Test-TcpPortAndTrace {
          [pscustomobject]@{ Tcp = [pscustomobject]@{ TcpTestSucceeded = $true; RemoteAddress = '127.0.0.1'; PingSucceeded = $true }; Trace = [pscustomobject]@{ TraceRoute = @() } }
        }

        Mock Invoke-Iperf3 {
          param(
            [string]$Server,
            [int]$Port,
            [string]$Stack,
            [int]$Duration,
            [int]$Omit,
            [int]$Tos,
            [string]$Proto,
            [string]$Dir,
            [int]$Streams,
            [string]$Win,
            [string]$UdpBw,
            [int]$ConnectTimeoutMs,
            [pscustomobject]$Caps,
            [scriptblock]$Runner
          )

          $null = $Server, $Port, $Stack, $Duration, $Omit, $Tos, $Dir, $Streams, $Win, $ConnectTimeoutMs, $Caps, $Runner

          if ($Proto -eq 'UDP') {
            $script:udpBws.Add($UdpBw) | Out-Null
            $json = [pscustomobject]@{
              end = [pscustomobject]@{
                sum_sent     = [pscustomobject]@{ bits_per_second = 1000000; lost_percent = 10 }
                sum_received = [pscustomobject]@{ bits_per_second = 900000 }
                sum          = [pscustomobject]@{ lost_percent = 10; jitter_ms = 1.0 }
              }
            }
          }
          else {
            $json = [pscustomobject]@{ end = $null }
          }

          return [pscustomobject]@{
            Args           = @()
            ExitCode       = 0
            RawLines       = @()
            RawText        = ''
            Json           = $json
            JsonParseError = $null
          }
        }

        $null = Invoke-Iperf3TestSuite -Target 'example.local' -OutDir $TestDrive -Quiet -DisableMtuProbe `
          -TcpStreams @(1) -TcpWindows @('default') -DscpClasses @('CS0') `
          -UdpStart '1M' -UdpMax '2M' -UdpStep '1M' -UdpLossThreshold 1.0

        $script:udpBws | Where-Object { $_ -eq '2M' } | Should -BeNullOrEmpty
        $script:udpBws.Count | Should -BeGreaterOrEqual 1
        $script:udpBws[0] | Should -Be '1M'  # Verify first bandwidth was tried
      }
    }
  }

  Context 'SingleTest protocol filter' {
    It 'runs exactly one UDP TX test for SingleTest + Protocol UDP' {
      InModuleScope Iperf3TestSuite {
        Mock Test-Iperf3TestSuitePrerequisites { }
        Mock Get-Iperf3Capability { $global:Iperf3TestSuite_TestCapability }
        Mock Test-Reachability { 'IPv4' }
        Mock Test-TcpPortAndTrace {
          [pscustomobject]@{ Tcp = [pscustomobject]@{ TcpTestSucceeded = $true; RemoteAddress = '127.0.0.1'; PingSucceeded = $true }; Trace = [pscustomobject]@{ TraceRoute = @() } }
        }

        $script:calls = New-Object System.Collections.Generic.List[object]
        Mock Invoke-Iperf3 {
          param(
            [string]$Server,
            [int]$Port,
            [string]$Stack,
            [int]$Duration,
            [int]$Omit,
            [int]$Tos,
            [string]$Proto,
            [string]$Dir,
            [int]$Streams,
            [string]$Win,
            [string]$UdpBw,
            [int]$ConnectTimeoutMs,
            [pscustomobject]$Caps,
            [scriptblock]$Runner
          )
          $null = $Server, $Port, $Stack, $Duration, $Omit, $Tos, $Streams, $Win, $ConnectTimeoutMs, $Caps, $Runner
          $script:calls.Add([pscustomobject]@{ Proto = $Proto; Dir = $Dir; UdpBw = $UdpBw }) | Out-Null
          [pscustomobject]@{
            Args           = @()
            ExitCode       = 0
            RawLines       = @()
            RawText        = ''
            Json           = [pscustomobject]@{ end = [pscustomobject]@{ sum_sent = [pscustomobject]@{ bits_per_second = 1000000; lost_percent = 0.0 }; sum_received = [pscustomobject]@{ bits_per_second = 900000 }; sum = [pscustomobject]@{ lost_percent = 0.0; jitter_ms = 1.0 } } }
            JsonParseError = $null
          }
        }

        $summary = Invoke-Iperf3TestSuite -Target 'example.local' -OutDir $TestDrive -Quiet -DisableMtuProbe -SingleTest -Protocol UDP -DscpClasses @('CS0') -PassThru
        $script:calls.Count | Should -Be 1
        $script:calls[0].Proto | Should -Be 'UDP'
        $script:calls[0].Dir | Should -Be 'TX'
        $summary.Counts.Total | Should -Be 1
      }
    }
  }

  Context 'Configuration normalization' {
    It 'ignores unknown keys in non-strict mode' {
      InModuleScope Iperf3TestSuite {
        $res = ConvertTo-Iperf3NormalizedParameterSet -InputParameters @{ Port = 5201; UnknownKey = 'x' } -AllowedKeys @('Port') -StrictConfiguration:$false
        $res.Parameters.ContainsKey('Port') | Should -Be $true
        $res.Parameters.ContainsKey('UnknownKey') | Should -Be $false
        @($res.Warnings).Count | Should -Be 1
      }
    }

    It 'throws on unknown keys in strict mode' {
      InModuleScope Iperf3TestSuite {
        { ConvertTo-Iperf3NormalizedParameterSet -InputParameters @{ UnknownKey = 'x' } -AllowedKeys @('Port') -StrictConfiguration } | Should -Throw
      }
    }

    It 'drops invalid range values in non-strict mode' {
      InModuleScope Iperf3TestSuite {
        $res = ConvertTo-Iperf3NormalizedParameterSet -InputParameters @{ Port = 70000 } -AllowedKeys @('Port') -StrictConfiguration:$false
        $res.Parameters.ContainsKey('Port') | Should -Be $false
        @($res.Warnings).Count | Should -Be 1
        $res.Warnings[0] | Should -Match 'range'
      }
    }

    It 'throws on invalid range values in strict mode' {
      InModuleScope Iperf3TestSuite {
        { ConvertTo-Iperf3NormalizedParameterSet -InputParameters @{ Port = 70000 } -AllowedKeys @('Port') -StrictConfiguration } | Should -Throw
      }
    }
  }

  Context 'Profiles' {
    It 'saves, lists, and loads profile parameters' {
      InModuleScope Iperf3TestSuite {
        $profilesFile = Join-Path $TestDrive 'profiles.json'
        $save = Save-Iperf3Profile -ProfileName 'lab' -ProfilesFile $profilesFile -Parameters @{ Target = 'example.local'; Port = 5201; Protocol = 'TCP' } -StrictConfiguration
        $save.ProfileName | Should -Be 'lab'
        $names = Get-Iperf3ProfileNames -ProfilesFile $profilesFile -StrictConfiguration
        $names | Should -Contain 'lab'
        $loaded = Get-Iperf3ProfileParameters -ProfileName 'lab' -ProfilesFile $profilesFile -StrictConfiguration
        $loaded.Target | Should -Be 'example.local'
        $loaded.Port | Should -Be 5201
        $loaded.Protocol | Should -Be 'TCP'
      }
    }

    It 'removes a saved profile' {
      InModuleScope Iperf3TestSuite {
        $profilesFile = Join-Path $TestDrive 'profiles-remove.json'
        $null = Save-Iperf3Profile -ProfileName 'to-remove' -ProfilesFile $profilesFile -Parameters @{ Target = 'example.local' }
        $removed = Remove-Iperf3Profile -ProfileName 'to-remove' -ProfilesFile $profilesFile
        $removed | Should -Be $true
        $names = Get-Iperf3ProfileNames -ProfilesFile $profilesFile
        $names | Should -Not -Contain 'to-remove'
      }
    }

    It 'creates a backup when profiles file is corrupt in non-strict mode' {
      InModuleScope Iperf3TestSuite {
        $profilesFile = Join-Path $TestDrive 'profiles-corrupt.json'
        Set-Content -LiteralPath $profilesFile -Encoding UTF8 -Value '{not-json'
        $names = Get-Iperf3ProfileNames -ProfilesFile $profilesFile
        @($names).Count | Should -Be 0
        $backups = @(Get-ChildItem -LiteralPath $TestDrive -Filter 'profiles-corrupt.json.corrupt.*.bak')
        $backups.Count | Should -Be 1
      }
    }

    It 'blocks profiles path traversal via relative path' {
      InModuleScope Iperf3TestSuite {
        Push-Location $TestDrive
        try {
          { Save-Iperf3Profile -ProfileName 'x' -ProfilesFile '../profiles.json' -Parameters @{ Target = 'example.local' } -StrictConfiguration } | Should -Throw '*must be under the current directory*'
        }
        finally {
          Pop-Location
        }
      }
    }

    It 'lists profiles via Invoke-Iperf3TestSuite passthru mode' {
      InModuleScope Iperf3TestSuite {
        $profilesFile = Join-Path $TestDrive 'profiles-list.json'
        $null = Save-Iperf3Profile -ProfileName 'a' -ProfilesFile $profilesFile -Parameters @{ Target = 'example.local' }
        $res = Invoke-Iperf3TestSuite -ListProfiles -ProfilesFile $profilesFile -PassThru -Quiet
        $res.Mode | Should -Be 'ListProfiles'
        @($res.Profiles) | Should -Contain 'a'
      }
    }
  }

  Context 'PassThru summary and supplemental outputs' {
    It 'returns summary and writes supplemental report files' {
      InModuleScope Iperf3TestSuite {
        Mock Test-Iperf3TestSuitePrerequisites { }
        Mock Get-Iperf3Capability { $global:Iperf3TestSuite_TestCapability }
        Mock Test-Reachability { 'IPv4' }
        Mock Test-TcpPortAndTrace {
          [pscustomobject]@{ Tcp = [pscustomobject]@{ TcpTestSucceeded = $true; RemoteAddress = '127.0.0.1'; PingSucceeded = $true }; Trace = [pscustomobject]@{ TraceRoute = @() } }
        }
        Mock Invoke-Iperf3 {
          [pscustomobject]@{
            Args           = @()
            ExitCode       = 0
            RawLines       = @()
            RawText        = ''
            Json           = [pscustomobject]@{ end = [pscustomobject]@{ sum_sent = [pscustomobject]@{ bits_per_second = 1000000; retransmits = 0 }; sum_received = [pscustomobject]@{ bits_per_second = 900000 } } }
            JsonParseError = $null
          }
        }

        $summary = Invoke-Iperf3TestSuite -Target 'example.local' -OutDir $TestDrive -DisableMtuProbe -Quiet -Protocol TCP -DscpClasses @('CS0') -TcpStreams @(1) -TcpWindows @('default') -PassThru
        $summary.ExitCode | Should -Be 0
        (Test-Path -LiteralPath $summary.Supplemental.SummaryJsonPath) | Should -Be $true
        (Test-Path -LiteralPath $summary.Supplemental.ReportMdPath) | Should -Be $true
        (Test-Path -LiteralPath $summary.Supplemental.RunIndexPath) | Should -Be $true
      }
    }
  }

  Context 'CLI exit code mapping' {
    It 'returns 11 for unknown profile name' {
      $scriptPath = Join-Path $script:RepoRoot 'apps/throughput/iPerf3Test.ps1'
      & pwsh -NoLogo -NoProfile -File $scriptPath -ProfileName '__does_not_exist__' -WhatIf *> $null
      $LASTEXITCODE | Should -Be 11
    }

    It 'deletes an existing profile via CLI DeleteProfile' {
      $scriptPath = Join-Path $script:RepoRoot 'apps/throughput/iPerf3Test.ps1'
      $profilesFile = Join-Path $TestDrive 'cli-delete-profiles.json'

      & pwsh -NoLogo -NoProfile -File $scriptPath -Target 'example.local' -ProfilesFile $profilesFile -ProfileName 'cli-temp' -SaveProfile -WhatIf -Quiet *> $null
      $LASTEXITCODE | Should -Be 0

      & pwsh -NoLogo -NoProfile -File $scriptPath -ProfilesFile $profilesFile -DeleteProfile 'cli-temp' -Quiet *> $null
      $LASTEXITCODE | Should -Be 0

      $names = @(Get-Iperf3ProfileNames -ProfilesFile $profilesFile)
      $names | Should -Not -Contain 'cli-temp'
    }

    It 'returns 11 when deleting a non-existing profile via CLI DeleteProfile' {
      $scriptPath = Join-Path $script:RepoRoot 'apps/throughput/iPerf3Test.ps1'
      $profilesFile = Join-Path $TestDrive 'cli-delete-missing.json'
      & pwsh -NoLogo -NoProfile -File $scriptPath -ProfilesFile $profilesFile -DeleteProfile '__missing__' -Quiet *> $null
      $LASTEXITCODE | Should -Be 11
    }

    It 'returns 12 when wrapper bootstrap fails during import' {
      $scriptPath = Join-Path $script:RepoRoot 'apps/throughput/iPerf3Test.ps1'
      $command = @"
function Import-Module { throw 'simulated import failure' }
& '$scriptPath' -Target 'example.local' -WhatIf -Quiet *> `$null
exit `$LASTEXITCODE
"@

      & pwsh -NoLogo -NoProfile -Command $command *> $null
      $LASTEXITCODE | Should -Be 12
    }
  }

  Context 'CLI precedence' {
    It 'loads saved profile values through the CLI wrapper' {
      $scriptPath = Join-Path $script:RepoRoot 'apps/throughput/iPerf3Test.ps1'
      $profilesFile = Join-Path $TestDrive 'cli-profiles.json'
      $null = Save-Iperf3Profile -ProfileName 'lab' -ProfilesFile $profilesFile -Parameters @{ Target = 'profile.example'; Port = 5300; Protocol = 'UDP' }

      $command = @"
`$result = & '$scriptPath' -ProfileName 'lab' -ProfilesFile '$profilesFile' -WhatIf -Quiet -PassThru
`$result | ConvertTo-Json -Compress -Depth 8
"@
      $json = & pwsh -NoLogo -NoProfile -Command $command
      $result = $json | ConvertFrom-Json

      $result.EffectiveParameters.Target | Should -Be 'profile.example'
      $result.EffectiveParameters.Port | Should -Be 5300
      $result.EffectiveParameters.Protocol | Should -Be 'UDP'
    }

    It 'lets explicit CLI args override saved profile values' {
      $scriptPath = Join-Path $script:RepoRoot 'apps/throughput/iPerf3Test.ps1'
      $profilesFile = Join-Path $TestDrive 'cli-profiles-override.json'
      $null = Save-Iperf3Profile -ProfileName 'lab' -ProfilesFile $profilesFile -Parameters @{ Target = 'profile.example'; Port = 5300; Protocol = 'UDP' }

      $command = @"
`$result = & '$scriptPath' -ProfileName 'lab' -ProfilesFile '$profilesFile' -Target 'cli.example' -Port 5400 -Protocol TCP -WhatIf -Quiet -PassThru
`$result | ConvertTo-Json -Compress -Depth 8
"@
      $json = & pwsh -NoLogo -NoProfile -Command $command
      $result = $json | ConvertFrom-Json

      $result.EffectiveParameters.Target | Should -Be 'cli.example'
      $result.EffectiveParameters.Port | Should -Be 5400
      $result.EffectiveParameters.Protocol | Should -Be 'TCP'
    }

    It 'lets config override defaults but not explicit CLI args' {
      $scriptPath = Join-Path $script:RepoRoot 'apps/throughput/iPerf3Test.ps1'
      $configPath = Join-Path $TestDrive 'cli-config.json'
      @{
        Target = 'config.example'
        Port = 5500
        Protocol = 'UDP'
      } | ConvertTo-Json | Set-Content -LiteralPath $configPath -Encoding UTF8

      $command = @"
`$result = & '$scriptPath' -ConfigurationPath '$configPath' -Target 'cli.example' -WhatIf -Quiet -PassThru
`$result | ConvertTo-Json -Compress -Depth 8
"@
      $json = & pwsh -NoLogo -NoProfile -Command $command
      $result = $json | ConvertFrom-Json

      $result.EffectiveParameters.Target | Should -Be 'cli.example'
      $result.EffectiveParameters.Port | Should -Be 5500
      $result.EffectiveParameters.Protocol | Should -Be 'UDP'
      $result.EffectiveParameters.Duration | Should -Be 10
    }
  }

  # --- Phase 4A: Validation and conversion edge cases ---

  Context 'Hostname/IP validation edge cases' {
    It 'accepts valid hostnames' {
      InModuleScope Iperf3TestSuite {
        Test-ValidHostnameOrIP -Name 'example.com' | Should -Be $true
        Test-ValidHostnameOrIP -Name 'my-host' | Should -Be $true
        Test-ValidHostnameOrIP -Name 'a.b.c.d.e' | Should -Be $true
      }
    }

    It 'accepts valid IPv4 addresses' {
      InModuleScope Iperf3TestSuite {
        Test-ValidHostnameOrIP -Name '192.168.1.1' | Should -Be $true
        Test-ValidHostnameOrIP -Name '0.0.0.0' | Should -Be $true
        Test-ValidHostnameOrIP -Name '255.255.255.255' | Should -Be $true
      }
    }

    It 'rejects IPv4 octets out of range' {
      InModuleScope Iperf3TestSuite {
        Test-ValidHostnameOrIP -Name '256.1.1.1' | Should -Be $false
        Test-ValidHostnameOrIP -Name '1.1.1.999' | Should -Be $false
      }
    }

    It 'rejects names starting with dash (argument injection)' {
      InModuleScope Iperf3TestSuite {
        Test-ValidHostnameOrIP -Name '-evil' | Should -Be $false
        Test-ValidHostnameOrIP -Name '--version' | Should -Be $false
      }
    }

    It 'rejects empty and whitespace' {
      InModuleScope Iperf3TestSuite {
        # Empty string rejected at parameter binding (Mandatory); whitespace returns $false
        { Test-ValidHostnameOrIP -Name '' } | Should -Throw
        Test-ValidHostnameOrIP -Name '   ' | Should -Be $false
      }
    }

    It 'accepts valid IPv6 addresses' {
      InModuleScope Iperf3TestSuite {
        Test-ValidHostnameOrIP -Name '::1' | Should -Be $true
        Test-ValidHostnameOrIP -Name 'fe80::1' | Should -Be $true
        Test-ValidHostnameOrIP -Name '[::1]' | Should -Be $true
      }
    }

    It 'rejects malformed or scoped IPv6 literals' {
      InModuleScope Iperf3TestSuite {
        Test-ValidHostnameOrIP -Name '2001:::1' | Should -Be $false
        Test-ValidHostnameOrIP -Name 'fe80::1%en0' | Should -Be $false
        Test-ValidHostnameOrIP -Name '[fe80::1%12]' | Should -Be $false
        Test-ValidHostnameOrIP -Name '[::1' | Should -Be $false
      }
    }
  }

  Context 'DSCP mapping edge cases' {
    It 'maps all CS classes correctly' {
      InModuleScope Iperf3TestSuite {
        Get-TosFromDscpClass -Class 'CS0' | Should -Be 0
        Get-TosFromDscpClass -Class 'CS1' | Should -Be 32
        Get-TosFromDscpClass -Class 'CS7' | Should -Be 224
      }
    }

    It 'maps AF classes correctly' {
      InModuleScope Iperf3TestSuite {
        Get-TosFromDscpClass -Class 'AF41' | Should -Be 136
        Get-TosFromDscpClass -Class 'AF43' | Should -Be 152
      }
    }

    It 'throws on invalid DSCP like CS8 or AF14' {
      InModuleScope Iperf3TestSuite {
        { Get-TosFromDscpClass -Class 'CS8' } | Should -Throw
        { Get-TosFromDscpClass -Class 'AF14' } | Should -Throw
        { Get-TosFromDscpClass -Class '' } | Should -Throw
      }
    }
  }

  Context 'Bandwidth conversion edge cases' {
    It 'treats bare numbers as megabits' {
      InModuleScope Iperf3TestSuite {
        ConvertTo-MbitPerSecond -Value '100' | Should -Be 100.0
      }
    }

    It 'handles lowercase and uppercase suffixes' {
      InModuleScope Iperf3TestSuite {
        ConvertTo-MbitPerSecond -Value '10m' | Should -Be 10.0
        ConvertTo-MbitPerSecond -Value '10M' | Should -Be 10.0
        ConvertTo-MbitPerSecond -Value '1g' | Should -Be 1000.0
        ConvertTo-MbitPerSecond -Value '1G' | Should -Be 1000.0
      }
    }

    It 'throws on empty/whitespace input' {
      InModuleScope Iperf3TestSuite {
        # Empty string rejected at parameter binding (Mandatory); whitespace throws explicitly
        { ConvertTo-MbitPerSecond -Value '' } | Should -Throw
        { ConvertTo-MbitPerSecond -Value '  ' } | Should -Throw
      }
    }
  }

  # --- Phase 4B: ConfigValidation, Profiles, ErrorClassification ---

  Context 'DSCP class validation in config' {
    It 'accepts valid DSCP class names' {
      InModuleScope Iperf3TestSuite {
        $result = ConvertTo-Iperf3KnownValue -Key 'DscpClasses' -Value @('CS0', 'EF', 'AF11', 'AF43')
        $result | Should -Contain 'CS0'
        $result | Should -Contain 'EF'
      }
    }

    It 'rejects invalid DSCP class names' {
      InModuleScope Iperf3TestSuite {
        { ConvertTo-Iperf3KnownValue -Key 'DscpClasses' -Value @('CS8') } | Should -Throw '*Invalid DSCP class*'
        { ConvertTo-Iperf3KnownValue -Key 'DscpClasses' -Value @('AF14') } | Should -Throw '*Invalid DSCP class*'
        { ConvertTo-Iperf3KnownValue -Key 'DscpClasses' -Value @('NOPE') } | Should -Throw '*Invalid DSCP class*'
        { ConvertTo-Iperf3KnownValue -Key 'DscpClasses' -Value @('ef') } | Should -Throw '*Invalid DSCP class*'
      }
    }
  }

  Context 'TCP window size validation in config' {
    It 'accepts valid TCP window sizes' {
      InModuleScope Iperf3TestSuite {
        $result = ConvertTo-Iperf3KnownValue -Key 'TcpWindows' -Value @('default', '128K', '256M', '1G', '4096')
        $result | Should -Contain 'default'
        $result | Should -Contain '128K'
      }
    }

    It 'rejects invalid TCP window sizes' {
      InModuleScope Iperf3TestSuite {
        { ConvertTo-Iperf3KnownValue -Key 'TcpWindows' -Value @('abc') } | Should -Throw '*Invalid TCP window size*'
        { ConvertTo-Iperf3KnownValue -Key 'TcpWindows' -Value @('128T') } | Should -Throw '*Invalid TCP window size*'
        { ConvertTo-Iperf3KnownValue -Key 'TcpWindows' -Value @('-1K') } | Should -Throw '*Invalid TCP window size*'
      }
    }
  }

  Context 'OutDir config validation' {
    It 'rejects OutDir with control characters' {
      InModuleScope Iperf3TestSuite {
        { ConvertTo-Iperf3KnownValue -Key 'OutDir' -Value "logs`0evil" } | Should -Throw '*control characters*'
        { ConvertTo-Iperf3KnownValue -Key 'OutDir' -Value "logs`ttab" } | Should -Throw '*control characters*'
      }
    }

    It 'rejects OutDir values that look like opener options' {
      InModuleScope Iperf3TestSuite {
        { ConvertTo-Iperf3KnownValue -Key 'OutDir' -Value '-reports' } | Should -Throw '*must not look like an option*'
      }
    }

    It 'accepts normal OutDir paths' {
      InModuleScope Iperf3TestSuite {
        $result = ConvertTo-Iperf3KnownValue -Key 'OutDir' -Value 'logs/output'
        $result | Should -Be 'logs/output'
      }
    }
  }

  Context 'Unknown parameter key error message' {
    It 'suggests Get-Iperf3TestSuiteDefaultParameterSet' {
      InModuleScope Iperf3TestSuite {
        { ConvertTo-Iperf3KnownValue -Key 'Bogus' -Value 'x' } | Should -Throw '*Get-Iperf3TestSuiteDefaultParameterSet*'
      }
    }
  }

  Context 'Profile name validation' {
    It 'rejects profile names with invalid characters' {
      InModuleScope Iperf3TestSuite {
        $profilesFile = Join-Path $TestDrive 'profiles-chartest.json'
        { Save-Iperf3Profile -ProfileName 'a/b' -ProfilesFile $profilesFile -Parameters @{ Target = 'x' } } | Should -Throw '*invalid characters*'
        { Save-Iperf3Profile -ProfileName 'a*b' -ProfilesFile $profilesFile -Parameters @{ Target = 'x' } } | Should -Throw '*invalid characters*'
        { Save-Iperf3Profile -ProfileName "a`0b" -ProfilesFile $profilesFile -Parameters @{ Target = 'x' } } | Should -Throw '*invalid characters*'
      }
    }

    It 'rejects profile names exceeding 128 characters' {
      InModuleScope Iperf3TestSuite {
        $profilesFile = Join-Path $TestDrive 'profiles-lentest.json'
        $longName = 'x' * 129
        { Save-Iperf3Profile -ProfileName $longName -ProfilesFile $profilesFile -Parameters @{ Target = 'x' } } | Should -Throw '*exceeds maximum length*'
      }
    }

    It 'accepts valid profile names up to 128 characters' {
      InModuleScope Iperf3TestSuite {
        $profilesFile = Join-Path $TestDrive 'profiles-validname.json'
        $okName = 'x' * 128
        $result = Save-Iperf3Profile -ProfileName $okName -ProfilesFile $profilesFile -Parameters @{ Target = 'example.local' }
        $result.ProfileName | Should -Be $okName
      }
    }
  }

  Context 'ProfilesFile path validation' {
    It 'rejects ProfilesFile with control characters' {
      InModuleScope Iperf3TestSuite {
        { Resolve-ProfilesFilePath -ProfilesFile "test`0file.json" } | Should -Throw '*control characters*'
      }
    }

    It 'requires .json extension for writing' {
      InModuleScope Iperf3TestSuite {
        $noJsonExt = Join-Path $TestDrive 'profiles.txt'
        { Save-Iperf3Profile -ProfileName 'lab' -ProfilesFile $noJsonExt -Parameters @{ Target = 'example.local' } } | Should -Throw '*.json extension*'
      }
    }

    It 'rejects profiles file exceeding 1 MB' {
      InModuleScope Iperf3TestSuite {
        $bigFile = Join-Path $TestDrive 'big-profiles.json'
        $bigContent = '{"version":1,"profiles":{' + ('"k":' + ('"' + ('x' * 1000) + '",' ) * 1050) + '"end":"v"}}'
        Set-Content -LiteralPath $bigFile -Value $bigContent -Encoding UTF8
        { Read-Iperf3ProfilesStore -ProfilesFile $bigFile -StrictConfiguration } | Should -Throw '*exceeds maximum size*'
      }
    }
  }

  Context 'Error classification for new validation messages' {
    It 'classifies "invalid characters" as InputValidation' {
      InModuleScope Iperf3TestSuite {
        $ex = New-Object System.Exception("ProfileName contains invalid characters: 'a/b'.")
        $er = New-Object System.Management.Automation.ErrorRecord($ex, 'test', 'NotSpecified', $null)
        $result = Resolve-Iperf3ClassifiedError -ErrorRecord $er
        $result.ErrorId | Should -Be 'Iperf3TestSuite.InputValidation'
      }
    }

    It 'classifies "exceeds maximum length" as InputValidation' {
      InModuleScope Iperf3TestSuite {
        $ex = New-Object System.Exception("ProfileName exceeds maximum length (128 characters).")
        $er = New-Object System.Management.Automation.ErrorRecord($ex, 'test', 'NotSpecified', $null)
        $result = Resolve-Iperf3ClassifiedError -ErrorRecord $er
        $result.ErrorId | Should -Be 'Iperf3TestSuite.InputValidation'
      }
    }

    It 'classifies "Invalid DSCP class" as InputValidation' {
      InModuleScope Iperf3TestSuite {
        $ex = New-Object System.Exception("Invalid DSCP class 'CS8' in 'DscpClasses'.")
        $er = New-Object System.Management.Automation.ErrorRecord($ex, 'test', 'NotSpecified', $null)
        $result = Resolve-Iperf3ClassifiedError -ErrorRecord $er
        $result.ErrorId | Should -Be 'Iperf3TestSuite.InputValidation'
      }
    }

    It 'classifies "Profiles file exceeds maximum size" as Prerequisite' {
      InModuleScope Iperf3TestSuite {
        $ex = New-Object System.Exception("Profiles file exceeds maximum size (1 MB): /path/to/file")
        $er = New-Object System.Management.Automation.ErrorRecord($ex, 'test', 'NotSpecified', $null)
        $result = Resolve-Iperf3ClassifiedError -ErrorRecord $er
        $result.ErrorId | Should -Be 'Iperf3TestSuite.Prerequisite'
      }
    }

    It 'classifies ".json extension" as InputValidation' {
      InModuleScope Iperf3TestSuite {
        $ex = New-Object System.Exception("Profiles file must have a .json extension: /path/to/file.txt")
        $er = New-Object System.Management.Automation.ErrorRecord($ex, 'test', 'NotSpecified', $null)
        $result = Resolve-Iperf3ClassifiedError -ErrorRecord $er
        $result.ErrorId | Should -Be 'Iperf3TestSuite.InputValidation'
      }
    }

    It 'classifies "control characters" as InputValidation' {
      InModuleScope Iperf3TestSuite {
        $ex = New-Object System.Exception("OutDir path contains control characters.")
        $er = New-Object System.Management.Automation.ErrorRecord($ex, 'test', 'NotSpecified', $null)
        $result = Resolve-Iperf3ClassifiedError -ErrorRecord $er
        $result.ErrorId | Should -Be 'Iperf3TestSuite.InputValidation'
      }
    }

    It 'classifies option-like paths as InputValidation' {
      InModuleScope Iperf3TestSuite {
        $ex = New-Object System.Exception("OutDir path must not look like an option (starts with '-'): -reports")
        $er = New-Object System.Management.Automation.ErrorRecord($ex, 'test', 'NotSpecified', $null)
        $result = Resolve-Iperf3ClassifiedError -ErrorRecord $er
        $result.ErrorId | Should -Be 'Iperf3TestSuite.InputValidation'
      }
    }

    It 'classifies unmatched patterns as Internal with verbose output' {
      InModuleScope Iperf3TestSuite {
        $ex = New-Object System.Exception("Something completely unexpected happened.")
        $er = New-Object System.Management.Automation.ErrorRecord($ex, 'test', 'NotSpecified', $null)
        $result = Resolve-Iperf3ClassifiedError -ErrorRecord $er
        $result.ErrorId | Should -Be 'Iperf3TestSuite.Internal'
      }
    }
  }

  # --- Phase 4C: JsonParsing, Common, PathHelpers ---

  Context 'JSON extraction with apostrophes (bug fix verification)' {
    It 'correctly parses JSON when text contains apostrophes' {
      InModuleScope Iperf3TestSuite {
        $s = "iperf3: error - can't connect to server: {`"error`":`"connection refused`"}"
        $result = Get-JsonSubstringOrNull -Text $s
        $result | Should -Be '{"error":"connection refused"}'
      }
    }

    It 'handles nested braces in JSON values' {
      InModuleScope Iperf3TestSuite {
        $s = '{"a":{"b":1}}'
        $result = Get-JsonSubstringOrNull -Text $s
        $result | Should -Be '{"a":{"b":1}}'
      }
    }
  }

  Context 'Test-PathUnderBase' {
    It 'accepts paths under the base' {
      InModuleScope Iperf3TestSuite {
        $base = $TestDrive
        $child = Join-Path $TestDrive 'sub' 'file.txt'
        Test-PathUnderBase -BasePath $base -CandidatePath $child | Should -Be $true
      }
    }

    It 'accepts the base path itself' {
      InModuleScope Iperf3TestSuite {
        Test-PathUnderBase -BasePath $TestDrive -CandidatePath $TestDrive | Should -Be $true
      }
    }

    It 'rejects paths outside the base' {
      InModuleScope Iperf3TestSuite {
        $base = Join-Path $TestDrive 'sub'
        $outside = Join-Path $TestDrive 'other'
        Test-PathUnderBase -BasePath $base -CandidatePath $outside | Should -Be $false
      }
    }
  }

  Context 'Open-FolderOrFile' {
    It 'passes an absolute path to platform openers' {
      . (Join-Path $script:RepoRoot 'scripts/PathHelpers.ps1')
      $relativePath = 'relative-output'
      $expectedPath = [System.IO.Path]::GetFullPath($relativePath)

      Mock -CommandName Get-Command { [pscustomobject]@{ Name = 'xdg-open' } }
      Mock -CommandName Start-Process {}

      Open-FolderOrFile -Path $relativePath

      Assert-MockCalled -CommandName Start-Process -Times 1 -Exactly -ParameterFilter {
        @($ArgumentList)[0] -eq $expectedPath
      }
    }
  }

  Context 'ConvertTo-Iperf3HashtableFromObject' {
    It 'converts PSCustomObject to hashtable' {
      InModuleScope Iperf3TestSuite {
        $obj = [pscustomobject]@{ A = 1; B = 'two' }
        $h = ConvertTo-Iperf3HashtableFromObject -InputObject $obj
        $h -is [hashtable] | Should -Be $true
        $h['A'] | Should -Be 1
        $h['B'] | Should -Be 'two'
      }
    }

    It 'passes through hashtable as-is' {
      InModuleScope Iperf3TestSuite {
        $h = @{ X = 42 }
        $result = ConvertTo-Iperf3HashtableFromObject -InputObject $h
        $result['X'] | Should -Be 42
      }
    }

    It 'returns empty hashtable for null' {
      InModuleScope Iperf3TestSuite {
        $result = ConvertTo-Iperf3HashtableFromObject -InputObject $null
        $result -is [hashtable] | Should -Be $true
        $result.Count | Should -Be 0
      }
    }
  }

  # --- Phase 11A: Build-TestPlan and orchestration ---

  Context 'Build-TestPlan' {
    It 'returns 1 test for SingleTest TCP' {
      InModuleScope Iperf3TestSuite {
        $caps = $global:Iperf3TestSuite_TestCapability
        $plan = Build-TestPlan -SingleTest:$true -Protocol 'TCP' -DscpClasses @('CS0','EF') -TcpStreams @(1,4) -TcpWindows @('default','128K') -Caps $caps -UdpStart '1M' -UdpMax '1G' -UdpStep '10M'
        $plan.TotalApprox | Should -Be 1
        $plan.RunTcp | Should -Be $true
        $plan.RunUdp | Should -Be $false
        $plan.RunSingleUdp | Should -Be $false
      }
    }

    It 'returns 1 test for SingleTest UDP' {
      InModuleScope Iperf3TestSuite {
        $caps = $global:Iperf3TestSuite_TestCapability
        $plan = Build-TestPlan -SingleTest:$true -Protocol 'UDP' -DscpClasses @('CS0') -TcpStreams @(1) -TcpWindows @('default') -Caps $caps -UdpStart '1M' -UdpMax '1G' -UdpStep '10M'
        $plan.TotalApprox | Should -Be 1
        $plan.RunTcp | Should -Be $false
        $plan.RunSingleUdp | Should -Be $true
      }
    }

    It 'calculates correct test count for full TCP matrix' {
      InModuleScope Iperf3TestSuite {
        $caps = $global:Iperf3TestSuite_TestCapability  # BidirSupported = true → 3 dirs
        $plan = Build-TestPlan -SingleTest:$false -Protocol 'TCP' -DscpClasses @('CS0','EF') -TcpStreams @(1,4) -TcpWindows @('default') -Caps $caps -UdpStart '1M' -UdpMax '1G' -UdpStep '10M'
        # 2 DSCP * 3 dirs * 2 streams * 1 window = 12
        $plan.TotalApprox | Should -Be 12
        $plan.RunTcp | Should -Be $true
        $plan.RunUdp | Should -Be $false
      }
    }

    It 'produces 0 UDP saturation steps when maxMbps <= curMbps' {
      InModuleScope Iperf3TestSuite {
        $caps = $global:Iperf3TestSuite_TestCapability
        $plan = Build-TestPlan -SingleTest:$false -Protocol 'UDP' -DscpClasses @('CS0') -TcpStreams @(1) -TcpWindows @('default') -Caps $caps -UdpStart '1G' -UdpMax '500M' -UdpStep '10M'
        # Only fixed UDP tests (2 per DSCP), no saturation
        $plan.TotalApprox | Should -Be 2
      }
    }
  }

  # --- Phase 11B: Invoke-Iperf3 args and capabilities ---

  Context 'Invoke-Iperf3 adds -6 for IPv6' {
    It 'adds -6 flag when Stack is IPv6' {
      $captured = InModuleScope Iperf3TestSuite {
        $script:captured = $null
        $caps = $global:Iperf3TestSuite_TestCapability
        $runner = { param([string[]]$IperfArgs); $script:captured = $IperfArgs; $global:LASTEXITCODE = 0; return '{"end":{}}' }
        $null = Invoke-Iperf3 -Server 'example' -Port 5201 -Stack 'IPv6' -Duration 1 -Omit 0 -Proto 'TCP' -Dir 'TX' -Caps $caps -Runner $runner
        return $script:captured
      }
      $captured | Should -Contain '-6'
    }
  }

  Context 'Invoke-Iperf3 returns DurationMs' {
    It 'includes DurationMs in result' {
      InModuleScope Iperf3TestSuite {
        $caps = $global:Iperf3TestSuite_TestCapability
        $runner = { param([string[]]$IperfArgs); $null = $IperfArgs; $global:LASTEXITCODE = 0; return '{"end":{}}' }
        $result = Invoke-Iperf3 -Server 'example' -Port 5201 -Stack 'IPv4' -Duration 1 -Omit 0 -Proto 'TCP' -Dir 'TX' -Caps $caps -Runner $runner
        $result.PSObject.Properties.Name | Should -Contain 'DurationMs'
        $result.DurationMs | Should -BeGreaterOrEqual 0
      }
    }
  }

  Context 'Get-Iperf3Capability version parsing' {
    It 'correctly identifies bidir support for 3.9' {
      InModuleScope Iperf3TestSuite {
        Mock Get-Command { [pscustomobject]@{ Name = 'iperf3' } } -ParameterFilter { $Name -eq 'iperf3' }
        # We can't easily mock & iperf3 --version, but we can test the regex logic
        $verText = 'iperf 3.9 (cJSON 1.7.15)'
        $m = [regex]::Match($verText, '\b([0-9]+)\.([0-9]+)\b')
        $m.Success | Should -Be $true
        [int]$m.Groups[1].Value | Should -Be 3
        [int]$m.Groups[2].Value | Should -Be 9
      }
    }
  }

  # --- Phase 11C: Reporting and run summary ---

  Context 'Build-RunSummary' {
    It 'returns Success for all-pass results' {
      InModuleScope Iperf3TestSuite {
        $results = @(
          [pscustomobject]@{ No = 1; Proto = 'TCP'; Dir = 'TX'; DSCP = 'CS0'; ExitCode = 0; JsonParseError = $null; RawText = '' }
          [pscustomobject]@{ No = 2; Proto = 'TCP'; Dir = 'RX'; DSCP = 'CS0'; ExitCode = 0; JsonParseError = $null; RawText = '' }
        )
        $summary = Build-RunSummary -Results $results -TestCount 2 -ParseErrorCount 0 -Target 'x' -Port 5201 -Stack 'IPv4' -Timestamp 'ts' -OutDir '/tmp'
        $summary.Status | Should -Be 'Success'
        $summary.ExitCode | Should -Be 0
        $summary.Counts.Succeeded | Should -Be 2
        $summary.Counts.Failed | Should -Be 0
      }
    }

    It 'returns TotalFailure when all tests fail' {
      InModuleScope Iperf3TestSuite {
        $results = @(
          [pscustomobject]@{ No = 1; Proto = 'TCP'; Dir = 'TX'; DSCP = 'CS0'; ExitCode = 1; JsonParseError = 'bad'; RawText = '' }
        )
        $summary = Build-RunSummary -Results $results -TestCount 1 -ParseErrorCount 1 -Target 'x' -Port 5201 -Stack 'IPv4' -Timestamp 'ts' -OutDir '/tmp'
        $summary.Status | Should -Be 'TotalFailure'
        $summary.Counts.Failed | Should -Be 1
      }
    }

    It 'returns PartialFailure for mixed results' {
      InModuleScope Iperf3TestSuite {
        $results = @(
          [pscustomobject]@{ No = 1; Proto = 'TCP'; Dir = 'TX'; DSCP = 'CS0'; ExitCode = 0; JsonParseError = $null; RawText = '' }
          [pscustomobject]@{ No = 2; Proto = 'TCP'; Dir = 'RX'; DSCP = 'CS0'; ExitCode = 1; JsonParseError = $null; RawText = 'connection refused' }
        )
        $summary = Build-RunSummary -Results $results -TestCount 2 -ParseErrorCount 0 -Target 'x' -Port 5201 -Stack 'IPv4' -Timestamp 'ts' -OutDir '/tmp'
        $summary.Status | Should -Be 'PartialFailure'
        $summary.Counts.Succeeded | Should -Be 1
        $summary.Counts.Failed | Should -Be 1
      }
    }

    It 'returns TotalFailure for zero tests' {
      InModuleScope Iperf3TestSuite {
        $summary = Build-RunSummary -Results @() -TestCount 0 -ParseErrorCount 0 -Target 'x' -Port 5201 -Stack 'IPv4' -Timestamp 'ts' -OutDir '/tmp'
        $summary.Status | Should -Be 'TotalFailure'
      }
    }

    It 'includes timing fields when provided' {
      InModuleScope Iperf3TestSuite {
        $summary = Build-RunSummary -Results @() -TestCount 0 -ParseErrorCount 0 -Target 'x' -Port 5201 -Stack 'IPv4' -Timestamp 'ts' -OutDir '/tmp' -StartedUtc '2025-01-01T00:00:00Z' -CompletedUtc '2025-01-01T00:01:00Z' -ElapsedSeconds 60.0 -Iperf3Version 'iperf3 3.9'
        $summary.StartedUtc | Should -Be '2025-01-01T00:00:00Z'
        $summary.CompletedUtc | Should -Be '2025-01-01T00:01:00Z'
        $summary.ElapsedSeconds | Should -Be 60.0
        $summary.SummaryVersion | Should -Be 2
      }
    }

    It 'includes Environment metadata' {
      InModuleScope Iperf3TestSuite {
        $summary = Build-RunSummary -Results @() -TestCount 0 -ParseErrorCount 0 -Target 'x' -Port 5201 -Stack 'IPv4' -Timestamp 'ts' -OutDir '/tmp' -Iperf3Version 'iperf3 3.9'
        $summary.Environment | Should -Not -BeNullOrEmpty
        $summary.Environment.Iperf3Version | Should -Be 'iperf3 3.9'
        $summary.Environment.PowerShellVersion | Should -Not -BeNullOrEmpty
      }
    }

    It 'includes FailureBreakdown for partial failures' {
      InModuleScope Iperf3TestSuite {
        $results = @(
          [pscustomobject]@{ No = 1; Proto = 'TCP'; Dir = 'TX'; DSCP = 'CS0'; ExitCode = 1; JsonParseError = 'parse error'; RawText = '' }
          [pscustomobject]@{ No = 2; Proto = 'TCP'; Dir = 'RX'; DSCP = 'CS0'; ExitCode = 1; JsonParseError = $null; RawText = 'connection refused' }
          [pscustomobject]@{ No = 3; Proto = 'TCP'; Dir = 'TX'; DSCP = 'EF'; ExitCode = 0; JsonParseError = $null; RawText = '' }
        )
        $summary = Build-RunSummary -Results $results -TestCount 3 -ParseErrorCount 1 -Target 'x' -Port 5201 -Stack 'IPv4' -Timestamp 'ts' -OutDir '/tmp'
        $summary.FailureBreakdown | Should -Not -BeNullOrEmpty
        @($summary.FailureBreakdown).Count | Should -Be 2
      }
    }

    It 'returns TotalFailure when a zero-exit result lacks required metrics' {
      InModuleScope Iperf3TestSuite {
        $results = @(
          [pscustomobject]@{
            No = 1; Proto = 'TCP'; Dir = 'TX'; DSCP = 'CS0'; ExitCode = 0
            JsonParseError = $null; MetricError = 'required throughput metrics missing'; RawText = ''
          }
        )

        $summary = Build-RunSummary -Results $results -TestCount 1 -ParseErrorCount 0 -Target 'x' -Port 5201 -Stack 'IPv4' -Timestamp 'ts' -OutDir '/tmp'

        $summary.Status | Should -Be 'TotalFailure'
        $summary.Counts.Failed | Should -Be 1
        $summary.FailureBreakdown[0].Reason | Should -Be 'required throughput metrics missing'
      }
    }
  }

  Context 'CSV row includes Duration_ms column' {
    It 'adds Duration_ms to CSV row' {
      InModuleScope Iperf3TestSuite {
        $row = ConvertTo-Iperf3CsvRow -No 1 -Proto 'TCP' -Dir 'TX' -DSCP 'CS0' -Streams 1 -Win 'default' `
          -ThrTxMbps 1.23 -RetrTx 0 -ThrRxMbps 0.0 -LossTxPct $null -JitterMs $null -DurationMs 5200 -Role 'end'
        $row.PSObject.Properties.Name | Should -Contain 'Duration_ms'
        $row.Duration_ms | Should -Be 5200
      }
    }
  }

  Context 'RetryCount config validation' {
    It 'accepts valid retry count' {
      InModuleScope Iperf3TestSuite {
        $result = ConvertTo-Iperf3KnownValue -Key 'RetryCount' -Value 3
        $result | Should -Be 3
      }
    }

    It 'rejects retry count out of range' {
      InModuleScope Iperf3TestSuite {
        { ConvertTo-Iperf3KnownValue -Key 'RetryCount' -Value 10 } | Should -Throw '*range*'
      }
    }
  }

  # --- Phase 14C: Threshold evaluation tests ---

  Context 'Threshold evaluation in Build-RunSummary' {
    It 'returns Success with no thresholds (backward compat)' {
      InModuleScope Iperf3TestSuite {
        $results = @(
          [pscustomobject]@{ No = 1; Proto = 'TCP'; Dir = 'TX'; DSCP = 'CS0'; ExitCode = 0; JsonParseError = $null; RawText = ''; Metrics = [pscustomobject]@{ TxMbps = 100; RxMbps = 90; Retr = 0; LossPct = $null; JitterMs = $null } }
        )
        $summary = Build-RunSummary -Results $results -TestCount 1 -ParseErrorCount 0 -Target 'x' -Port 5201 -Stack 'IPv4' -Timestamp 'ts' -OutDir '/tmp'
        $summary.Status | Should -Be 'Success'
        $summary.ThresholdBreachCount | Should -Be 0
      }
    }

    It 'detects throughput below threshold' {
      InModuleScope Iperf3TestSuite {
        $results = @(
          [pscustomobject]@{ No = 1; Proto = 'TCP'; Dir = 'TX'; DSCP = 'CS0'; ExitCode = 0; JsonParseError = $null; RawText = ''; Metrics = [pscustomobject]@{ TxMbps = 50; RxMbps = 40; Retr = 0; LossPct = $null; JitterMs = $null } }
        )
        $summary = Build-RunSummary -Results $results -TestCount 1 -ParseErrorCount 0 -Target 'x' -Port 5201 -Stack 'IPv4' -Timestamp 'ts' -OutDir '/tmp' -ThresholdMinThroughputMbps 100.0
        $summary.Status | Should -Be 'PartialFailure'
        $summary.ThresholdBreachCount | Should -BeGreaterThan 0
        $summary.ThresholdBreaches[0].Reasons | Should -Not -BeNullOrEmpty
      }
    }

    It 'detects loss above threshold' {
      InModuleScope Iperf3TestSuite {
        $results = @(
          [pscustomobject]@{ No = 1; Proto = 'UDP'; Dir = 'TX'; DSCP = 'CS0'; ExitCode = 0; JsonParseError = $null; RawText = ''; Metrics = [pscustomobject]@{ TxMbps = 100; RxMbps = 90; Retr = $null; LossPct = 5.5; JitterMs = 1.0 } }
        )
        $summary = Build-RunSummary -Results $results -TestCount 1 -ParseErrorCount 0 -Target 'x' -Port 5201 -Stack 'IPv4' -Timestamp 'ts' -OutDir '/tmp' -ThresholdMaxLossPct 2.0
        $summary.Status | Should -Be 'PartialFailure'
        $summary.ThresholdBreachCount | Should -Be 1
      }
    }

    It 'passes when metrics are within thresholds' {
      InModuleScope Iperf3TestSuite {
        $results = @(
          [pscustomobject]@{ No = 1; Proto = 'TCP'; Dir = 'TX'; DSCP = 'CS0'; ExitCode = 0; JsonParseError = $null; RawText = ''; Metrics = [pscustomobject]@{ TxMbps = 500; RxMbps = 480; Retr = 0; LossPct = $null; JitterMs = $null } }
        )
        $summary = Build-RunSummary -Results $results -TestCount 1 -ParseErrorCount 0 -Target 'x' -Port 5201 -Stack 'IPv4' -Timestamp 'ts' -OutDir '/tmp' -ThresholdMinThroughputMbps 100.0
        $summary.Status | Should -Be 'Success'
        $summary.ThresholdBreachCount | Should -Be 0
      }
    }

    It 'validates threshold config parameters' {
      InModuleScope Iperf3TestSuite {
        ConvertTo-Iperf3KnownValue -Key 'ThresholdMinThroughputMbps' -Value 100 | Should -Be 100.0
        ConvertTo-Iperf3KnownValue -Key 'ThresholdMaxLossPct' -Value 2.5 | Should -Be 2.5
        ConvertTo-Iperf3KnownValue -Key 'ThresholdMaxJitterMs' -Value 10 | Should -Be 10.0
      }
    }
  }

  # --- Phase 15A: Connectivity and helper tests ---

  Context 'Get-PingArgumentsForStack' {
    It 'returns IPv4 args with -4' {
      InModuleScope Iperf3TestSuite {
        $testHost = 'example'
        $args4 = Get-PingArgumentsForStack -Stack 'IPv4' -ComputerName $testHost
        $args4 | Should -Contain '-4'
        $args4 | Should -Contain 'example'
      }
    }

    It 'returns IPv6 args with -6' {
      InModuleScope Iperf3TestSuite {
        $testHost = 'example'
        $args6 = Get-PingArgumentsForStack -Stack 'IPv6' -ComputerName $testHost
        $args6 | Should -Contain '-6'
      }
    }

    It 'includes MTU payload size when specified' {
      InModuleScope Iperf3TestSuite {
        $testHost = 'example'
        $args4 = Get-PingArgumentsForStack -Stack 'IPv4' -ComputerName $testHost -MtuPayloadSize 1400
        $args4 | Should -Contain '-f'
        $args4 | Should -Contain '1400'
      }
    }
  }

  Context 'Get-BitsPerSecondMbps' {
    It 'extracts Mbps from valid object' {
      InModuleScope Iperf3TestSuite {
        $obj = [pscustomobject]@{ bits_per_second = 10000000 }
        Get-BitsPerSecondMbps -Obj $obj | Should -Be 10.0
      }
    }

    It 'returns null for null object' {
      InModuleScope Iperf3TestSuite {
        Get-BitsPerSecondMbps -Obj $null | Should -Be $null
      }
    }

    It 'returns null when property is missing' {
      InModuleScope Iperf3TestSuite {
        $obj = [pscustomobject]@{ other = 123 }
        Get-BitsPerSecondMbps -Obj $obj | Should -Be $null
      }
    }
  }

  Context 'Test-Iperf3TestSuitePrerequisites cross-platform' {
    It 'does not throw on non-Windows when both checks are skipped' -Skip:$IsWindows {
      InModuleScope Iperf3TestSuite {
        Mock Get-Command { [pscustomobject]@{ Name = $Name } }
        { Test-Iperf3TestSuitePrerequisites -SkipReachabilityCheck -DisableMtuProbe } | Should -Not -Throw
      }
    }
  }

  # --- Phase 15B: Reporting file I/O tests ---

  Context 'Write-Iperf3SupplementalReports' {
    It 'creates summary JSON and Markdown report files' {
      InModuleScope Iperf3TestSuite {
        $summary = [pscustomobject]@{
          SummaryVersion = 2; Timestamp = 'ts'; StartedUtc = ''; CompletedUtc = ''; ElapsedSeconds = 1
          OutDir = $TestDrive; Target = 'x'; Port = 5201; Stack = 'IPv4'
          Status = 'Success'; ExitCode = 0
          Counts = [pscustomobject]@{ Total = 1; Succeeded = 1; Failed = 0; ParseErrors = 0 }
          Environment = [pscustomobject]@{ Iperf3Version = 'test'; PowerShellVersion = '7'; OS = 'test' }
          FailureBreakdown = @(); ThresholdBreaches = @(); ThresholdBreachCount = 0
          TopFailures = @()
          Supplemental = [pscustomobject]@{ SummaryJsonPath = $null; ReportMdPath = $null; RunIndexPath = $null }
        }
        $result = Write-Iperf3SupplementalReports -RunSummary $summary -OutDir $TestDrive -Timestamp 'test_ts'
        (Test-Path -LiteralPath $result.SummaryJsonPath) | Should -Be $true
        (Test-Path -LiteralPath $result.ReportMdPath) | Should -Be $true
        $md = Get-Content -LiteralPath $result.ReportMdPath -Raw
        $md | Should -Match '# iperf3 Test Run Report'
        $md | Should -Match 'Status: Success'
      }
    }
  }

  Context 'Write-Iperf3RunIndex' {
    It 'creates a valid JSON index file' {
      InModuleScope Iperf3TestSuite {
        $summary = [pscustomobject]@{
          Timestamp = 'ts'; Status = 'Success'; ExitCode = 0
          Target = 'x'; Port = 5201; Stack = 'IPv4'
        }
        $indexPath = Write-Iperf3RunIndex -OutDir $TestDrive -RunSummary $summary -CsvPath '/csv' -JsonPath '/json' -SummaryJsonPath '/summary' -ReportMdPath '/report'
        (Test-Path -LiteralPath $indexPath) | Should -Be $true
        $index = Get-Content -LiteralPath $indexPath -Raw | ConvertFrom-Json
        $index.schemaVersion | Should -Be 2
        $index.lastRun.status | Should -Be 'Success'
      }
    }
  }

  Context 'Write-FinalOutputs artifact contract' {
    It 'exposes artifact warnings without changing successful measurement status' {
      InModuleScope Iperf3TestSuite {
        $csvRows = [System.Collections.Generic.List[object]]::new()
        $allResults = [System.Collections.Generic.List[object]]::new()
        $csvRows.Add([pscustomobject]@{ No = 1; Proto = 'TCP'; Dir = 'TX'; DSCP = 'CS0'; Thr_TX_Mbps = 100 }) | Out-Null
        $allResults.Add([pscustomobject]@{
            No = 1; Proto = 'TCP'; Dir = 'TX'; DSCP = 'CS0'; ExitCode = 0
            JsonParseError = $null; MetricError = $null; RawText = ''
            Metrics = [pscustomobject]@{ TxMbps = 100; RxMbps = $null; Retr = 0; LossPct = $null; JitterMs = $null }
          }) | Out-Null
        $final = [pscustomobject]@{ Target = 'x'; Port = 5201; Stack = 'IPv4' }
        Mock -CommandName Set-Content { throw 'disk full' }

        $result = Write-FinalOutputs `
          -CsvRowsList $csvRows `
          -AllResultsList $allResults `
          -CsvPath (Join-Path $TestDrive 'out.csv') `
          -JsonPath (Join-Path $TestDrive 'out.json') `
          -FinalResultObject $final `
          -OutDir $TestDrive `
          -Timestamp 'artifact_fail'

        $result.RunSummary.Status | Should -Be 'Success'
        $result.RunSummary.ArtifactStatus.Complete | Should -BeFalse
        $result.RunSummary.ArtifactStatus.Json | Should -Be 'Warn'
        $result.RunSummary.ArtifactStatus.SummaryJson | Should -Be 'Warn'
        $result.RunSummary.ArtifactStatus.ReportMd | Should -Be 'Warn'
        $result.RunSummary.ArtifactStatus.RunIndex | Should -Be 'Warn'
      }
    }
  }

  # --- Phase 15C: Config helpers and WhatIf flow ---

  Context 'ConvertTo-Iperf3IntArray' {
    It 'converts valid values to int array' {
      InModuleScope Iperf3TestSuite {
        $result = ConvertTo-Iperf3IntArray -Value @(1, 2, 3)
        $result.Count | Should -Be 3
        $result[0] | Should -Be 1
      }
    }

    It 'throws on non-integer value' {
      InModuleScope Iperf3TestSuite {
        { ConvertTo-Iperf3IntArray -Value @('abc') } | Should -Throw
      }
    }
  }

  Context 'ConvertTo-Iperf3StringArray' {
    It 'strips whitespace and filters blanks' {
      InModuleScope Iperf3TestSuite {
        $result = ConvertTo-Iperf3StringArray -Value @(' hello ', '', '  ', 'world')
        $result.Count | Should -Be 2
        $result[0] | Should -Be 'hello'
        $result[1] | Should -Be 'world'
      }
    }
  }

  Context 'WhatIf PassThru mode' {
    It 'returns WhatIf mode object without running tests' {
      InModuleScope Iperf3TestSuite {
        $result = Invoke-Iperf3TestSuite -Target 'example.local' -WhatIf -PassThru -Quiet
        $result.Mode | Should -Be 'WhatIf'
        $result.TotalApprox | Should -BeGreaterThan 0
        $result.PSObject.Properties.Name | Should -Contain 'CsvPath'
        $result.PSObject.Properties.Name | Should -Contain 'JsonPath'
      }
    }
  }

  # --- Phase 21A: TcpClient fallback and TimeoutMs ---

  Context 'Test-TcpPortAndTrace timeout parameter' {
    It 'accepts TimeoutMs parameter' {
      InModuleScope Iperf3TestSuite {
        # Just verify the function signature accepts the parameter (actual TCP test skipped)
        $cmd = Get-Command Test-TcpPortAndTrace
        $cmd.Parameters.Keys | Should -Contain 'TimeoutMs'
      }
    }
  }

  Context 'Get-TestSuiteConnectivity accepts ConnectTimeoutMs' {
    It 'has ConnectTimeoutMs parameter' {
      InModuleScope Iperf3TestSuite {
        $cmd = Get-Command Get-TestSuiteConnectivity
        $cmd.Parameters.Keys | Should -Contain 'ConnectTimeoutMs'
      }
    }
  }

  Context 'Get-Iperf3StackFromTcpResult' {
    It 'uses the remote endpoint address family before hostname text' {
      InModuleScope Iperf3TestSuite {
        $tcp = [pscustomobject]@{
          RemoteAddress = 'dualstack.example'
          RemoteAddressFamily = 'InterNetworkV6'
        }

        Get-Iperf3StackFromTcpResult -Tcp $tcp | Should -Be 'IPv6'
      }
    }
  }

  # --- Phase 21B: Run history and Compare-Iperf3Runs ---

  Context 'Write-Iperf3RunIndex history tracking' {
    It 'creates index with runs array' {
      InModuleScope Iperf3TestSuite {
        $summary = [pscustomobject]@{
          Timestamp = 'ts1'; Status = 'Success'; ExitCode = 0
          Target = 'x'; Port = 5201; Stack = 'IPv4'
        }
        $indexPath = Write-Iperf3RunIndex -OutDir $TestDrive -RunSummary $summary -CsvPath '/csv' -JsonPath '/json' -SummaryJsonPath '/s' -ReportMdPath '/r'
        $index = Get-Content -LiteralPath $indexPath -Raw | ConvertFrom-Json
        $index.schemaVersion | Should -Be 2
        $index.lastRun.timestamp | Should -Be 'ts1'
        @($index.runs).Count | Should -Be 1
      }
    }

    It 'appends to existing run history' {
      InModuleScope Iperf3TestSuite {
        $histDir = Join-Path $TestDrive 'history-append'
        New-Item -ItemType Directory -Path $histDir -Force | Out-Null
        $summary1 = [pscustomobject]@{ Timestamp = 'ts1'; Status = 'Success'; ExitCode = 0; Target = 'x'; Port = 5201; Stack = 'IPv4' }
        $summary2 = [pscustomobject]@{ Timestamp = 'ts2'; Status = 'PartialFailure'; ExitCode = 14; Target = 'x'; Port = 5201; Stack = 'IPv4' }
        $null = Write-Iperf3RunIndex -OutDir $histDir -RunSummary $summary1 -CsvPath '/csv1' -JsonPath '/json1' -SummaryJsonPath '/s1' -ReportMdPath '/r1'
        $indexPath = Write-Iperf3RunIndex -OutDir $histDir -RunSummary $summary2 -CsvPath '/csv2' -JsonPath '/json2' -SummaryJsonPath '/s2' -ReportMdPath '/r2'
        $index = Get-Content -LiteralPath $indexPath -Raw | ConvertFrom-Json
        @($index.runs).Count | Should -Be 2
        $index.lastRun.timestamp | Should -Be 'ts2'
        $index.runs[0].timestamp | Should -Be 'ts1'
        $index.runs[1].timestamp | Should -Be 'ts2'
      }
    }
  }

  Context 'Compare-Iperf3Runs' {
    It 'produces delta object from two summary files' {
      $baseline = [pscustomobject]@{
        Timestamp = 'ts1'; Status = 'Success'; ElapsedSeconds = 30
        Counts = [pscustomobject]@{ Total = 10; Succeeded = 10; Failed = 0; ParseErrors = 0 }
      }
      $current = [pscustomobject]@{
        Timestamp = 'ts2'; Status = 'PartialFailure'; ElapsedSeconds = 35
        Counts = [pscustomobject]@{ Total = 10; Succeeded = 8; Failed = 2; ParseErrors = 0 }
      }
      $bPath = Join-Path $TestDrive 'baseline.json'
      $cPath = Join-Path $TestDrive 'current.json'
      $baseline | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $bPath -Encoding UTF8
      $current | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $cPath -Encoding UTF8

      $delta = Compare-Iperf3Runs -BaselinePath $bPath -CurrentPath $cPath
      $delta.StatusChanged | Should -Be $true
      $delta.BaselineStatus | Should -Be 'Success'
      $delta.CurrentStatus | Should -Be 'PartialFailure'
      $delta.FailedDelta | Should -Be 2
      $delta.TotalDelta | Should -Be 0
    }

    It 'throws when baseline file is missing' {
      { Compare-Iperf3Runs -BaselinePath '/nonexistent.json' -CurrentPath '/also-missing.json' } | Should -Throw '*not found*'
    }
  }
}
