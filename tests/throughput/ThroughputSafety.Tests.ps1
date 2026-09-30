Set-StrictMode -Version Latest

BeforeAll {
  $script:RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
  $script:ModuleManifest = Join-Path $script:RepoRoot 'src/powershell/throughput/NetworkLantern.Throughput.psd1'
  Import-Module $script:ModuleManifest -Force
}

Describe 'NetworkLantern.Throughput bounded native output' {
  It 'retains exact-limit output without reporting truncation' {
    InModuleScope 'NetworkLantern.Throughput' {
      $pwshPath = (Get-Process -Id $PID).Path
      $command = "[Console]::Out.Write(('x' * 1MB)); [Console]::Error.Write(('y' * 64KB))"
      $result = Invoke-Iperf3NativeProcess -FilePath $pwshPath -Arguments @('-NoProfile', '-NonInteractive', '-Command', $command) -TimeoutMs 10000

      [Text.Encoding]::UTF8.GetByteCount($result.StdOut) | Should -Be 1MB
      [Text.Encoding]::UTF8.GetByteCount($result.StdErr) | Should -Be 64KB
      $result.StdOutTruncated | Should -BeFalse
      $result.StdErrTruncated | Should -BeFalse
      $result.NativeOutputTruncated | Should -BeFalse
      $result.StreamsCompleted | Should -BeTrue
    }
  }

  It 'loads one reusable async reader type and concurrently drains both streams to their byte caps' {
    $readerType = [NetworkLantern.Throughput.Native.BoundedStreamReader]
    Import-Module $script:ModuleManifest -Force
    [NetworkLantern.Throughput.Native.BoundedStreamReader] | Should -Be $readerType
    InModuleScope 'NetworkLantern.Throughput' {
      $pwshPath = (Get-Process -Id $PID).Path
      $command = @'
Add-Type -TypeDefinition @"
using System;
using System.IO;
using System.Threading.Tasks;
public static class ConcurrentConsoleWriter {
  private static void Fill(Stream stream, byte value, int count) {
    byte[] block = new byte[8192];
    for (int i = 0; i < block.Length; i++) block[i] = value;
    while (count > 0) {
      int write = Math.Min(count, block.Length);
      stream.Write(block, 0, write);
      count -= write;
    }
    stream.Flush();
  }
  public static void Run(int stdoutBytes, int stderrBytes) {
    Task stdout = Task.Run(() => Fill(Console.OpenStandardOutput(), 120, stdoutBytes));
    Task stderr = Task.Run(() => Fill(Console.OpenStandardError(), 121, stderrBytes));
    Task.WaitAll(stdout, stderr);
  }
}
"@
[ConcurrentConsoleWriter]::Run(1MB + 17, 64KB + 17)
'@
      $result = Invoke-Iperf3NativeProcess -FilePath $pwshPath -Arguments @('-NoProfile', '-NonInteractive', '-Command', $command) -TimeoutMs 10000

      [Text.Encoding]::UTF8.GetByteCount($result.StdOut) | Should -Be 1MB
      [Text.Encoding]::UTF8.GetByteCount($result.StdErr) | Should -Be 64KB
      $result.StdOutTruncated | Should -BeTrue
      $result.StdErrTruncated | Should -BeTrue
      $result.NativeOutputTruncated | Should -BeTrue
      $result.StreamsCompleted | Should -BeTrue
    }
  }

  It 'fails a measurement when native output overflows even if the captured prefix contains valid JSON' {
    InModuleScope 'NetworkLantern.Throughput' {
      $pwshPath = (Get-Process -Id $PID).Path
      $caps = [pscustomobject]@{ BidirSupported = $true; VersionText = 'fixture' }
      $command = '$json = ''{"end":{"sum_sent":{"bits_per_second":2500000},"sum_received":{"bits_per_second":1750000}}}''; [Console]::Out.Write($json + (''x'' * 1MB))'
      $result = Invoke-Iperf3 -Server example.local -Port 5201 -Stack IPv4 -Duration 1 -Omit 0 -Proto TCP -Dir TX -Caps $caps -Iperf3Path $pwshPath -NativeArgumentsOverride @('-NoProfile', '-NonInteractive', '-Command', $command)

      $result.Json.end.sum_sent.bits_per_second | Should -Be 2500000
      $result.ExitCode | Should -Be -1
      $result.StdOutTruncated | Should -BeTrue
      $result.StdErrTruncated | Should -BeFalse
      $result.NativeOutputTruncated | Should -BeTrue
      $result.RawTextTruncated | Should -BeTrue
    }
  }

  It 'does not let a split terminal UTF-8 sequence expand decoded output beyond the native byte cap' {
    InModuleScope 'NetworkLantern.Throughput' {
      $pwshPath = (Get-Process -Id $PID).Path
      $command = '$bytes = [byte[]]::new(1MB + 3); [Array]::Fill($bytes, [byte]120); $bytes[1MB - 1] = 0xF0; $bytes[1MB] = 0x9F; $bytes[1MB + 1] = 0x98; $bytes[1MB + 2] = 0x80; $stream = [Console]::OpenStandardOutput(); $stream.Write($bytes, 0, $bytes.Length)'
      $result = Invoke-Iperf3NativeProcess -FilePath $pwshPath -Arguments @('-NoProfile', '-NonInteractive', '-Command', $command) -TimeoutMs 10000

      [Text.Encoding]::UTF8.GetByteCount($result.StdOut) | Should -BeLessOrEqual 1MB
      $result.StdOut | Should -Not -Match ([char]0xFFFD)
      $result.StdOutTruncated | Should -BeTrue
      $result.NativeOutputTruncated | Should -BeTrue
    }
  }

  It 'retains at most 16 KiB of valid UTF-8 diagnostics after parsing JSON' {
    InModuleScope 'NetworkLantern.Throughput' {
      $pwshPath = (Get-Process -Id $PID).Path
      $caps = [pscustomobject]@{ BidirSupported = $true; VersionText = 'fixture' }
      $command = '$json = ''{"end":{"sum_sent":{"bits_per_second":2500000},"sum_received":{"bits_per_second":1750000}}}''; [Console]::Out.Write($json + (''😀'' * 6000))'
      $result = Invoke-Iperf3 -Server example.local -Port 5201 -Stack IPv4 -Duration 1 -Omit 0 -Proto TCP -Dir TX -Caps $caps -Iperf3Path $pwshPath -NativeArgumentsOverride @('-NoProfile', '-NonInteractive', '-Command', $command)

      $result.ExitCode | Should -Be 0
      $result.Json.end.sum_received.bits_per_second | Should -Be 1750000
      [Text.Encoding]::UTF8.GetByteCount($result.RawText) | Should -BeLessOrEqual 16KB
      $result.RawText | Should -Not -Match ([char]0xFFFD)
      $result.NativeOutputTruncated | Should -BeFalse
      $result.RawTextTruncated | Should -BeTrue
    }
  }
}

Describe 'NetworkLantern.Throughput exact planning and budgets' {
  It 'returns exact default counts and estimates with and without bidirectional support' {
    InModuleScope 'NetworkLantern.Throughput' {
      $defaults = Get-NetworkThroughputDefaultParameterSet
      $bidir = Build-TestPlan -SingleTest:$false -Protocol Both -DscpClasses $defaults.DscpClasses -TcpStreams $defaults.TcpStreams -TcpWindows $defaults.TcpWindows -Caps ([pscustomobject]@{ BidirSupported = $true }) -UdpStart $defaults.UdpStart -UdpMax $defaults.UdpMax -UdpStep $defaults.UdpStep -Duration $defaults.Duration -Omit $defaults.Omit
      $oneWay = Build-TestPlan -SingleTest:$false -Protocol Both -DscpClasses $defaults.DscpClasses -TcpStreams $defaults.TcpStreams -TcpWindows $defaults.TcpWindows -Caps ([pscustomobject]@{ BidirSupported = $false }) -UdpStart $defaults.UdpStart -UdpMax $defaults.UdpMax -UdpStep $defaults.UdpStep -Duration $defaults.Duration -Omit $defaults.Omit

      $bidir.TotalApprox | Should -Be 1145
      $bidir.EstimatedTestSeconds | Should -Be 12595
      $bidir.DirsTcpList | Should -Be @('TX', 'RX', 'BD')
      $oneWay.TotalApprox | Should -Be 1100
      $oneWay.DirsTcpList | Should -Be @('TX', 'RX')
    }
  }

  It 'uses floor division for UDP saturation and does not repeat an equal fixed bound' {
    InModuleScope 'NetworkLantern.Throughput' {
      $caps = [pscustomobject]@{ BidirSupported = $true }
      $fractional = Build-TestPlan -SingleTest:$false -Protocol UDP -DscpClasses @('CS0') -TcpStreams @(1) -TcpWindows @('default') -Caps $caps -UdpStart 1M -UdpMax 2.5M -UdpStep 1M
      $equal = Build-TestPlan -SingleTest:$false -Protocol UDP -DscpClasses @('CS0') -TcpStreams @(1) -TcpWindows @('default') -Caps $caps -UdpStart 1M -UdpMax 1M -UdpStep 1M

      $fractional.TotalApprox | Should -Be 6
      $equal.TotalApprox | Should -Be 2

      $results = [Collections.Generic.List[object]]::new()
      $csv = [Collections.Generic.List[object]]::new()
      $results.Add([pscustomobject]@{ Fixture = $true })
      $csv.Add([pscustomobject]@{ Fixture = $true })
      $number = 0
      Mock Invoke-SingleIperf3TestAndAddResult {
        [pscustomobject]@{
          Run = [pscustomobject]@{ ExitCode = 0; Json = [pscustomobject]@{} }
          Metrics = [pscustomobject]@{ LossPct = 0 }
        }
      }
      Invoke-UdpSaturationForDscp -AllResultsList $results -CsvRowsList $csv -TestNoRef ([ref]$number) -Dscp CS0 -Tos 0 -Stack IPv4 -Target example.local -Port 5201 -Duration 1 -Omit 0 -ConnectTimeoutMs 1000 -Caps $caps -UdpLossThreshold 5 -CurMbps $fractional.CurMbps -MaxMbps $fractional.MaxMbps -StepMbps $fractional.StepMbps -TotalApprox $fractional.TotalApprox
      $number | Should -Be ($fractional.TotalApprox - 2)
      Should -Invoke Invoke-SingleIperf3TestAndAddResult -Exactly 4

      $decimalEdge = Build-TestPlan -SingleTest:$false -Protocol UDP -DscpClasses @('CS0') -TcpStreams @(1) -TcpWindows @('default') -Caps $caps -UdpStart 1.1M -UdpMax 4.4M -UdpStep 1.1M
      $number = 0
      Invoke-UdpSaturationForDscp -AllResultsList $results -CsvRowsList $csv -TestNoRef ([ref]$number) -Dscp CS0 -Tos 0 -Stack IPv4 -Target example.local -Port 5201 -Duration 1 -Omit 0 -ConnectTimeoutMs 1000 -Caps $caps -UdpLossThreshold 5 -CurMbps $decimalEdge.CurMbps -MaxMbps $decimalEdge.MaxMbps -StepMbps $decimalEdge.StepMbps -TotalApprox $decimalEdge.TotalApprox
      $decimalEdge.TotalApprox | Should -Be 10
      $number | Should -Be ($decimalEdge.TotalApprox - 2)
      Should -Invoke Invoke-SingleIperf3TestAndAddResult -Exactly 12
    }
  }

  It 'allows previews over budget and reports default, equality, and estimated duration' {
    InModuleScope 'NetworkLantern.Throughput' {
      Mock Get-Iperf3Capability { [pscustomobject]@{ VersionText = 'iperf 3.15'; Major = 3; Minor = 15; BidirSupported = $true } }
      $defaultPreview = Measure-NetworkThroughput -Target example.local -WhatIf -PassThru -Quiet
      $equalPreview = Measure-NetworkThroughput -Target example.local -Protocol TCP -SingleTest -Duration 3 -Omit 2 -MaxTotalTests 1 -WhatIf -PassThru -Quiet
      $overPreview = Measure-NetworkThroughput -Target example.local -MaxTotalTests 1 -WhatIf -PassThru -Quiet

      $defaultPreview.MaxTotalTests | Should -Be 0
      $defaultPreview.WithinTestBudget | Should -BeTrue
      $equalPreview.TotalApprox | Should -Be 1
      $equalPreview.EstimatedTestSeconds | Should -Be 5
      $equalPreview.WithinTestBudget | Should -BeTrue
      $overPreview.WithinTestBudget | Should -BeFalse
    }
  }

  It 'rejects an over-budget live plan before connectivity and output directory creation' {
    InModuleScope 'NetworkLantern.Throughput' {
      $outDir = Join-Path $TestDrive 'must-not-exist'
      Mock Test-NetworkThroughputPrerequisites {}
      Mock Get-Iperf3Capability { [pscustomobject]@{ VersionText = 'iperf 3.15'; Major = 3; Minor = 15; BidirSupported = $true } }
      Mock Get-TestSuiteConnectivity { throw 'connectivity must not run' }

      $errorRecord = $null
      try { Measure-NetworkThroughput -Target example.local -MaxTotalTests 1 -OutDir $outDir -PassThru -Quiet }
      catch { $errorRecord = $_ }

      (($errorRecord.FullyQualifiedErrorId -split ',')[0]) | Should -Be 'NetworkLantern.Throughput.InputValidation'
      $errorRecord.Exception.Message | Should -Match '1145.*exceeds MaxTotalTests 1'
      Should -Invoke Get-Iperf3Capability -Exactly 1
      Should -Invoke Get-TestSuiteConnectivity -Exactly 0
      Test-Path -LiteralPath $outDir | Should -BeFalse
    }
  }

  It 'honors profile then explicit parameter precedence for MaxTotalTests' {
    InModuleScope 'NetworkLantern.Throughput' {
      Mock Get-Iperf3Capability { [pscustomobject]@{ VersionText = 'iperf 3.15'; Major = 3; Minor = 15; BidirSupported = $true } }
      $profiles = Join-Path $TestDrive 'budget-profiles.json'
      $null = Save-Iperf3Profile -ProfileName budget -ProfilesFile $profiles -Parameters @{ Target = 'example.local'; MaxTotalTests = 2 } -StrictConfiguration

      $fromProfile = Measure-NetworkThroughput -ProfileName budget -ProfilesFile $profiles -WhatIf -PassThru -Quiet
      $explicit = Measure-NetworkThroughput -ProfileName budget -ProfilesFile $profiles -MaxTotalTests 3 -WhatIf -PassThru -Quiet
      $fromProfile.MaxTotalTests | Should -Be 2
      $explicit.MaxTotalTests | Should -Be 3
    }
  }

  It 'keeps complete controlled execution equal to planned totals for defaults and repeated entries' {
    InModuleScope 'NetworkLantern.Throughput' {
      Mock Test-NetworkThroughputPrerequisites {}
      Mock Get-TestSuiteConnectivity {
        [pscustomobject]@{
          Stack = 'IPv4'; MtuFails = @()
          Net = [pscustomobject]@{
            Tcp = [pscustomobject]@{ TcpTestSucceeded = $true; RemoteAddress = '127.0.0.1'; PingSucceeded = $true }
            Trace = $null
          }
        }
      }
      Mock Get-Iperf3Capability {
        [pscustomobject]@{ VersionText = 'iperf 3.15'; Major = 3; Minor = 15; BidirSupported = $script:testBidir }
      }
      Mock Invoke-SingleIperf3TestAndAddResult {
        param($AllResultsList, $CsvRowsList, $No, $Proto, $Dir, $DSCP, $Tos, $Streams, $Window, $UdpBw, $Stack, $Target, $Port)
        $run = [pscustomobject]@{ ExitCode = 0; Args = @(); RawText = ''; DurationMs = 1; JsonParseError = $null }
        $metrics = [pscustomobject]@{ TxMbps = 10.0; RxMbps = 10.0; Retr = 0; LossPct = 0.0; JitterMs = 0.0 }
        Add-Iperf3TestResult -AllResultsList $AllResultsList -CsvRowsList $CsvRowsList -No $No -Proto $Proto -Dir $Dir -DSCP $DSCP -Tos $Tos -Streams $Streams -Window $Window -UdpBw $UdpBw -Stack $Stack -Target $Target -Port $Port -Run $run -Metrics $metrics
        [pscustomobject]@{ Run = $run; Metrics = $metrics }
      }

      $script:testBidir = $true
      $bidir = Measure-NetworkThroughput -Target example.local -SkipReachabilityCheck -DisableMtuProbe -OutDir (Join-Path $TestDrive 'bidir') -PassThru -Quiet
      $bidir.Counts.Total | Should -Be 1145

      $script:testBidir = $false
      $oneWay = Measure-NetworkThroughput -Target example.local -SkipReachabilityCheck -DisableMtuProbe -OutDir (Join-Path $TestDrive 'one-way') -PassThru -Quiet
      $oneWay.Counts.Total | Should -Be 1100

      $script:testBidir = $true
      $repeated = Measure-NetworkThroughput -Target example.local -Protocol TCP -DscpClasses @('CS0', 'CS0') -TcpStreams @(1, 1) -TcpWindows @('default', 'default') -SkipReachabilityCheck -DisableMtuProbe -OutDir (Join-Path $TestDrive 'repeated') -PassThru -Quiet
      $repeated.Counts.Total | Should -Be 24
    }
  }

  It 'accepts live plans at exact and unlimited budgets' {
    InModuleScope 'NetworkLantern.Throughput' {
      Mock Test-NetworkThroughputPrerequisites {}
      Mock Get-Iperf3Capability { [pscustomobject]@{ VersionText = 'iperf 3.15'; Major = 3; Minor = 15; BidirSupported = $true } }
      Mock Get-TestSuiteConnectivity {
        [pscustomobject]@{
          Stack = 'IPv4'; MtuFails = @()
          Net = [pscustomobject]@{
            Tcp = [pscustomobject]@{ TcpTestSucceeded = $true; RemoteAddress = '127.0.0.1'; PingSucceeded = $true }
            Trace = $null
          }
        }
      }
      Mock Invoke-SingleIperf3TestAndAddResult {
        param($AllResultsList, $CsvRowsList, $No, $Proto, $Dir, $DSCP, $Tos, $Streams, $Window, $UdpBw, $Stack, $Target, $Port)
        $run = [pscustomobject]@{ ExitCode = 0; Args = @(); RawText = ''; DurationMs = 1; JsonParseError = $null }
        $metrics = [pscustomobject]@{ TxMbps = 10.0; RxMbps = 10.0; Retr = 0; LossPct = $null; JitterMs = $null }
        Add-Iperf3TestResult -AllResultsList $AllResultsList -CsvRowsList $CsvRowsList -No $No -Proto $Proto -Dir $Dir -DSCP $DSCP -Tos $Tos -Streams $Streams -Window $Window -UdpBw $UdpBw -Stack $Stack -Target $Target -Port $Port -Run $run -Metrics $metrics
        [pscustomobject]@{ Run = $run; Metrics = $metrics }
      }

      $exact = Measure-NetworkThroughput -Target example.local -Protocol TCP -SingleTest -MaxTotalTests 1 -SkipReachabilityCheck -DisableMtuProbe -OutDir (Join-Path $TestDrive 'exact') -PassThru -Quiet
      $unlimited = Measure-NetworkThroughput -Target example.local -Protocol TCP -SingleTest -MaxTotalTests 0 -SkipReachabilityCheck -DisableMtuProbe -OutDir (Join-Path $TestDrive 'unlimited') -PassThru -Quiet
      $exact.Status | Should -Be Success
      $exact.Counts.Total | Should -Be 1
      $unlimited.Status | Should -Be Success
      $unlimited.Counts.Total | Should -Be 1
    }
  }
}

Describe 'Throughput application budget surfaces and atomic artifacts' {
  It 'round-trips MaxTotalTests through a saved profile and the GUI binding helpers' {
    . (Join-Path $script:RepoRoot 'apps/throughput/Private/GuiBudget.ps1')
    $control = [pscustomobject]@{ Value = [decimal]0 }
    $controls = [pscustomobject]@{ ByName = @{ numMaxTotalTests = $control } }
    $controls | Add-Member -MemberType ScriptMethod -Name Find -Value { param($name, $searchChildren) $null = $searchChildren; @($this.ByName[$name]) }
    $form = [pscustomobject]@{ Controls = $controls }

    Set-GuiMaxTotalTests -Form $form -Value 2500
    $profiles = Join-Path $TestDrive 'gui-budget-profiles.json'
    $null = Save-Iperf3Profile -ProfileName gui -ProfilesFile $profiles -Parameters @{
      Target = 'example.local'
      MaxTotalTests = Get-GuiMaxTotalTests -Form $form
    } -StrictConfiguration
    $saved = Get-Iperf3ProfileParameters -ProfileName gui -ProfilesFile $profiles -StrictConfiguration
    Set-GuiMaxTotalTests -Form $form -Value 0
    Set-GuiMaxTotalTests -Form $form -Value ([int]$saved.MaxTotalTests)

    Get-GuiMaxTotalTests -Form $form | Should -Be 2500
  }

  It 'lets an explicit CLI MaxTotalTests override configuration' {
    $cli = Join-Path $script:RepoRoot 'apps/throughput/Measure-NetworkThroughput.ps1'
    $config = Join-Path $TestDrive 'budget-config.json'
    [IO.File]::WriteAllText($config, '{"Target":"example.local","MaxTotalTests":2}')

    $priorCli = $env:NETWORK_LANTERN_TEST_CLI
    $priorConfig = $env:NETWORK_LANTERN_TEST_CONFIG
    try {
      $env:NETWORK_LANTERN_TEST_CLI = $cli
      $env:NETWORK_LANTERN_TEST_CONFIG = $config
      $json = & pwsh -NoProfile -NonInteractive -Command '& $env:NETWORK_LANTERN_TEST_CLI -ConfigurationPath $env:NETWORK_LANTERN_TEST_CONFIG -MaxTotalTests 3 -WhatIf -PassThru -Quiet | ConvertTo-Json -Depth 6 -Compress'
      $LASTEXITCODE | Should -Be 0
      $result = $json | ConvertFrom-Json
      $result.MaxTotalTests | Should -Be 3
      $result.EffectiveParameters.MaxTotalTests | Should -Be 3
    }
    finally {
      $env:NETWORK_LANTERN_TEST_CLI = $priorCli
      $env:NETWORK_LANTERN_TEST_CONFIG = $priorConfig
    }
  }

  It 'preserves existing primary CSV JSON and Markdown files when atomic publication fails' {
    InModuleScope 'NetworkLantern.Throughput' {
      $timestamp = 'atomic_failure'
      $csvPath = Join-Path $TestDrive 'primary.csv'
      $jsonPath = Join-Path $TestDrive 'primary.json'
      $reportPath = Join-Path $TestDrive "iperf3_report_$timestamp.md"
      [IO.File]::WriteAllText($csvPath, 'old csv')
      [IO.File]::WriteAllText($jsonPath, 'old json')
      [IO.File]::WriteAllText($reportPath, 'old markdown')
      Mock Invoke-Iperf3AtomicReplace { throw 'publication blocked' }

      $csvRows = [Collections.Generic.List[object]]::new()
      $csvRows.Add([pscustomobject]@{ No = 1 })
      $results = [Collections.Generic.List[object]]::new()
      $final = [pscustomobject]@{ Target = 'example.local'; Port = 5201; Stack = 'IPv4'; Results = @() }
      $written = Write-FinalOutputs -CsvRowsList $csvRows -AllResultsList $results -CsvPath $csvPath -JsonPath $jsonPath -FinalResultObject $final -OutDir $TestDrive -Timestamp $timestamp

      [IO.File]::ReadAllText($csvPath) | Should -Be 'old csv'
      [IO.File]::ReadAllText($jsonPath) | Should -Be 'old json'
      [IO.File]::ReadAllText($reportPath) | Should -Be 'old markdown'
      $written.RunSummary.ArtifactStatus.Csv | Should -Be Warn
      $written.RunSummary.ArtifactStatus.Json | Should -Be Warn
      $written.RunSummary.ArtifactStatus.ReportMd | Should -Be Warn
      @(Get-ChildItem -LiteralPath $TestDrive -Filter '.*.tmp').Count | Should -Be 0
    }
  }

  It 'does not delete a foreign temp file when exclusive creation collides' {
    InModuleScope 'NetworkLantern.Throughput' {
      $destination = Join-Path $TestDrive 'collision.json'
      $foreignTemp = Join-Path $TestDrive '.collision.json.foreign.tmp'
      [IO.File]::WriteAllText($destination, 'old destination')
      [IO.File]::WriteAllText($foreignTemp, 'foreign temp')
      Mock Get-Iperf3AtomicTempPath { $foreignTemp }

      { Set-Iperf3TextFileAtomic -Path $destination -Text 'new destination' } | Should -Throw
      [IO.File]::ReadAllText($destination) | Should -Be 'old destination'
      [IO.File]::ReadAllText($foreignTemp) | Should -Be 'foreign temp'
    }
  }

  It 'preserves the destination and cleans its owned temp after a partial write failure' {
    InModuleScope 'NetworkLantern.Throughput' {
      $destination = Join-Path $TestDrive 'partial-write.json'
      [IO.File]::WriteAllText($destination, 'old destination')
      Mock Invoke-Iperf3AtomicTempWrite {
        param($Stream, $Bytes)
        $prefixLength = [math]::Min(4, $Bytes.Length)
        $Stream.Write($Bytes, 0, $prefixLength)
        throw 'injected partial write failure'
      }

      { Set-Iperf3TextFileAtomic -Path $destination -Text 'new destination' } | Should -Throw '*injected partial write failure*'
      [IO.File]::ReadAllText($destination) | Should -Be 'old destination'
      @(Get-ChildItem -LiteralPath $TestDrive -Filter '.*.tmp').Count | Should -Be 0
    }
  }
}
