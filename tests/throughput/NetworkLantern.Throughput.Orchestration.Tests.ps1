$ErrorActionPreference = 'Stop'

BeforeAll {
  . (Join-Path (Get-Item $PSScriptRoot).Parent.Parent.FullName 'scripts/Get-RepoRoot.ps1')
  $repoRoot = Get-RepoRoot
  $script:RepoRoot = $repoRoot
  $modulePath = Join-Path $repoRoot 'src/powershell/throughput/NetworkLantern.Throughput.psd1'
  Import-Module $modulePath -Force
  $script:TestCapability = [pscustomobject]@{ VersionText = 'iperf3 3.9'; Major = 3; Minor = 9; BidirSupported = $true }
  $global:NetworkThroughput_TestCapability = $script:TestCapability
  try {
    $global:IsWindows = $true
  } catch {
    Write-Verbose 'On some hosts (e.g. macOS) $IsWindows is read-only; Windows-only tests may fail or be skipped.'
  }
}

Describe 'Network Lantern throughput helpers' {

  Context 'Failure handling' {
    It 'emits InputValidation ErrorId when target is missing' {
      InModuleScope 'NetworkLantern.Throughput' {
        $err = $null
        try { Measure-NetworkThroughput -OutDir $TestDrive -Quiet } catch { $err = $_ }
        $err | Should -Not -BeNullOrEmpty
        $err.FullyQualifiedErrorId | Should -Match 'NetworkLantern.Throughput.InputValidation'
      }
    }

    It 'throws when reachability fails' {
      InModuleScope 'NetworkLantern.Throughput' {
        Mock Test-NetworkThroughputPrerequisites { }
        Mock Get-Command { [pscustomobject]@{ Name = $Name } }
        Mock Get-Iperf3Capability { $global:NetworkThroughput_TestCapability }
        Mock Test-Reachability { 'None' }
        Mock Test-TcpPortAndTrace { throw 'Should not be called' }
        Mock Invoke-Iperf3 { throw 'Should not be called' }

        Should -Throw -ActualValue { Measure-NetworkThroughput -Target 'example.local' -OutDir $TestDrive -Quiet } -ExpectedMessage "ICMP reachability*"
      }
    }

    It 'emits Connectivity ErrorId when reachability fails' {
      InModuleScope 'NetworkLantern.Throughput' {
        Mock Test-NetworkThroughputPrerequisites { }
        Mock Get-Command { [pscustomobject]@{ Name = $Name } }
        Mock Get-Iperf3Capability { $global:NetworkThroughput_TestCapability }
        Mock Test-Reachability { 'None' }
        Mock Test-TcpPortAndTrace { throw 'Should not be called' }
        Mock Invoke-Iperf3 { throw 'Should not be called' }

        $err = $null
        try { Measure-NetworkThroughput -Target 'example.local' -OutDir $TestDrive -Quiet } catch { $err = $_ }
        $err | Should -Not -BeNullOrEmpty
        $err.FullyQualifiedErrorId | Should -Match 'NetworkLantern.Throughput.Connectivity'
      }
    }

    It 'throws on non-Windows' -Skip:$IsWindows {
      InModuleScope 'NetworkLantern.Throughput' {
        Mock Get-Command { [pscustomobject]@{ Name = $Name } }
        Mock Get-Iperf3Capability { $global:NetworkThroughput_TestCapability }
        Mock Invoke-Iperf3 { throw 'Should not be called' }
        Should -Throw -ActualValue { Measure-NetworkThroughput -Target 'example.local' -OutDir $TestDrive -Quiet } -ExpectedMessage "*require Windows*"
      }
    }

    It 'throws when TCP port is not reachable' {
      InModuleScope 'NetworkLantern.Throughput' {
        Mock Test-NetworkThroughputPrerequisites { }
        Mock Get-Command { [pscustomobject]@{ Name = $Name } }
        Mock Get-Iperf3Capability { $global:NetworkThroughput_TestCapability }
        Mock Test-Reachability { 'IPv4' }
        Mock Test-TcpPortAndTrace {
          [pscustomobject]@{ Tcp = [pscustomobject]@{ TcpTestSucceeded = $false; RemoteAddress = '127.0.0.1'; PingSucceeded = $true }; Trace = $null }
        }
        Mock Invoke-Iperf3 { throw 'Should not be called' }

        Should -Throw -ActualValue { Measure-NetworkThroughput -Target 'example.local' -OutDir $TestDrive -Quiet } -ExpectedMessage "TCP port*"
      }
    }

    It 'throws when SingleTest and DscpClasses is empty' {
      InModuleScope 'NetworkLantern.Throughput' {
        Mock Test-NetworkThroughputPrerequisites { }
        Mock Get-Command { [pscustomobject]@{ Name = $Name } }
        Mock Get-Iperf3Capability { $global:NetworkThroughput_TestCapability }

        # Parameter binding may reject empty array; otherwise our explicit check throws
        $err = $null
        try { Measure-NetworkThroughput -Target 'example.local' -OutDir $TestDrive -Quiet -SingleTest -DscpClasses @() } catch { $err = $_ }
        $err | Should -Not -BeNullOrEmpty
        ($err.Exception.Message -match 'at least one DSCP|empty|null') | Should -Be $true
      }
    }
  }

  Context 'UDP saturation loop' {
    It 'stops when loss exceeds threshold' {
      InModuleScope 'NetworkLantern.Throughput' {
        Mock Test-NetworkThroughputPrerequisites { }
        Mock Get-Command { [pscustomobject]@{ Name = $Name } }
        $script:udpBws = New-Object System.Collections.Generic.List[string]

        Mock Get-Iperf3Capability { $global:NetworkThroughput_TestCapability }
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

        $null = Measure-NetworkThroughput -Target 'example.local' -OutDir $TestDrive -Quiet -DisableMtuProbe `
          -TcpStreams @(1) -TcpWindows @('default') -DscpClasses @('CS0') `
          -UdpStart '1M' -UdpMax '2M' -UdpStep '1M' -UdpLossThreshold 1.0

        $script:udpBws | Where-Object { $_ -eq '2M' } | Should -BeNullOrEmpty
        $script:udpBws.Count | Should -BeGreaterOrEqual 1
        $script:udpBws[0] | Should -Be '1M'  # Verify first bandwidth was tried
      }
    }

    It 'executes no saturation runs when UdpMax is not above UdpStart' {
      InModuleScope 'NetworkLantern.Throughput' {
        Mock Test-NetworkThroughputPrerequisites { }
        Mock Get-Iperf3Capability { $global:NetworkThroughput_TestCapability }
        Mock Test-Reachability { 'IPv4' }
        Mock Test-TcpPortAndTrace {
          [pscustomobject]@{
            Tcp = [pscustomobject]@{ TcpTestSucceeded = $true; RemoteAddress = '127.0.0.1'; PingSucceeded = $true }
            Trace = [pscustomobject]@{ TraceRoute = @() }
          }
        }
        Mock Invoke-Iperf3 {
          [pscustomobject]@{
            Args = @(); ExitCode = 0; RawLines = @(); RawText = ''; JsonParseError = $null; DurationMs = 1
            Json = [pscustomobject]@{
              end = [pscustomobject]@{
                sum_sent = [pscustomobject]@{ bits_per_second = 1000000; lost_percent = 0.0 }
                sum_received = [pscustomobject]@{ bits_per_second = 900000 }
                sum = [pscustomobject]@{ lost_percent = 0.0; jitter_ms = 1.0 }
              }
            }
          }
        }

        $outDir = Join-Path $TestDrive 'udp-no-saturation'
        $progressMessages = @()
        $summary = Measure-NetworkThroughput -Target 'example.local' -OutDir $outDir -Quiet -DisableMtuProbe `
          -Protocol UDP -DscpClasses @('CS0') -UdpStart '1G' -UdpMax '500M' -UdpStep '10M' `
          -Progress -PassThru -InformationVariable progressMessages

        Should -Invoke Invoke-Iperf3 -Times 2 -Exactly
        $summary.Counts.Total | Should -Be 2
        $runProgress = @($progressMessages | Where-Object { $_.MessageData -match '^Running test ' })
        $runProgress.Count | Should -Be 2
        $runProgress[0].MessageData | Should -Match '^Running test 1/2 '
        $runProgress[1].MessageData | Should -Match '^Running test 2/2 '
        ($runProgress.MessageData -join "`n") | Should -Not -Match 'UDP saturation'
      }
    }
  }

  Context 'SingleTest protocol filter' {
    It 'runs exactly one UDP TX test for SingleTest + Protocol UDP' {
      InModuleScope 'NetworkLantern.Throughput' {
        Mock Test-NetworkThroughputPrerequisites { }
        Mock Get-Iperf3Capability { $global:NetworkThroughput_TestCapability }
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

        $summary = Measure-NetworkThroughput -Target 'example.local' -OutDir $TestDrive -Quiet -DisableMtuProbe -SingleTest -Protocol UDP -DscpClasses @('CS0') -PassThru
        $script:calls.Count | Should -Be 1
        $script:calls[0].Proto | Should -Be 'UDP'
        $script:calls[0].Dir | Should -Be 'TX'
        $summary.Counts.Total | Should -Be 1
      }
    }
  }

  Context 'PassThru summary and supplemental outputs' {
    It 'returns summary and writes supplemental report files' {
      InModuleScope 'NetworkLantern.Throughput' {
        Mock Test-NetworkThroughputPrerequisites { }
        Mock Get-Iperf3Capability { $global:NetworkThroughput_TestCapability }
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

        $summary = Measure-NetworkThroughput -Target 'example.local' -OutDir $TestDrive -DisableMtuProbe -Quiet -Protocol TCP -DscpClasses @('CS0') -TcpStreams @(1) -TcpWindows @('default') -PassThru
        $summary.ExitCode | Should -Be 0
        (Test-Path -LiteralPath $summary.Supplemental.SummaryJsonPath) | Should -Be $true
        (Test-Path -LiteralPath $summary.Supplemental.ReportMdPath) | Should -Be $true
        (Test-Path -LiteralPath $summary.Supplemental.RunIndexPath) | Should -Be $true
      }
    }
  }

  Context 'Build-TestPlan' {
    It 'returns 1 test for SingleTest TCP' {
      InModuleScope 'NetworkLantern.Throughput' {
        $caps = $global:NetworkThroughput_TestCapability
        $plan = Build-TestPlan -SingleTest:$true -Protocol 'TCP' -DscpClasses @('CS0','EF') -TcpStreams @(1,4) -TcpWindows @('default','128K') -Caps $caps -UdpStart '1M' -UdpMax '1G' -UdpStep '10M'
        $plan.TotalApprox | Should -Be 1
        $plan.RunTcp | Should -Be $true
        $plan.RunUdp | Should -Be $false
        $plan.RunSingleUdp | Should -Be $false
      }
    }

    It 'returns 1 test for SingleTest UDP' {
      InModuleScope 'NetworkLantern.Throughput' {
        $caps = $global:NetworkThroughput_TestCapability
        $plan = Build-TestPlan -SingleTest:$true -Protocol 'UDP' -DscpClasses @('CS0') -TcpStreams @(1) -TcpWindows @('default') -Caps $caps -UdpStart '1M' -UdpMax '1G' -UdpStep '10M'
        $plan.TotalApprox | Should -Be 1
        $plan.RunTcp | Should -Be $false
        $plan.RunSingleUdp | Should -Be $true
      }
    }

    It 'calculates correct test count for full TCP matrix' {
      InModuleScope 'NetworkLantern.Throughput' {
        $caps = $global:NetworkThroughput_TestCapability  # BidirSupported = true → 3 dirs
        $plan = Build-TestPlan -SingleTest:$false -Protocol 'TCP' -DscpClasses @('CS0','EF') -TcpStreams @(1,4) -TcpWindows @('default') -Caps $caps -UdpStart '1M' -UdpMax '1G' -UdpStep '10M'
        # 2 DSCP * 3 dirs * 2 streams * 1 window = 12
        $plan.TotalApprox | Should -Be 12
        $plan.RunTcp | Should -Be $true
        $plan.RunUdp | Should -Be $false
      }
    }

    It 'produces 0 UDP saturation steps when maxMbps <= curMbps' {
      InModuleScope 'NetworkLantern.Throughput' {
        $caps = $global:NetworkThroughput_TestCapability
        $plan = Build-TestPlan -SingleTest:$false -Protocol 'UDP' -DscpClasses @('CS0') -TcpStreams @(1) -TcpWindows @('default') -Caps $caps -UdpStart '1G' -UdpMax '500M' -UdpStep '10M'
        # Only fixed UDP tests (2 per DSCP), no saturation
        $plan.TotalApprox | Should -Be 2
      }
    }
  }
}
