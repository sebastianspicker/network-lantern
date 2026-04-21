Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

BeforeAll {
  . "$PSScriptRoot/../../src/powershell/path/lib-ps/Get-RoundDefinitions.ps1"
  . "$PSScriptRoot/../../src/powershell/path/lib-ps/Invoke-HostDiagnostics.ps1"
  . "$PSScriptRoot/../../src/powershell/path/lib-ps/Save-DiagnosticResults.ps1"

  function Invoke-PingRaw {}
  function Invoke-TracertRaw {}
  function Invoke-PathpingRaw {}
  function Test-TcpPort {}
  function Test-UdpPortBestEffort {}
  function Write-Status {}
}

Describe 'NetPathSuite status modeling' {
  It 'marks pathping as skipped instead of successful when SkipPathping is set' {
    Mock Invoke-PingRaw { [pscustomobject]@{ Raw = @('ping'); ExitCode = 0 } }
    Mock Invoke-TracertRaw { [pscustomobject]@{ Raw = @('tracert'); ExitCode = 0 } }
    Mock Invoke-PathpingRaw { throw 'Pathping should not be called when skipped.' }
    Mock Test-TcpPort { [pscustomobject]@{ TcpTestSucceeded = $true; TraceRoute = @() } }
    Mock Test-UdpPortBestEffort { [pscustomobject]@{ Status = 'Likely reachable'; Detail = 'ok' } }

    $round = (Get-RoundDefinitions -TraceMaxHops 30 -TraceTimeoutMs 5000 -PathpingProbes 50 -PathpingTimeoutMs 3000 | Select-Object -First 1)
    $result = Invoke-HostDiagnostics -PlanItem ([pscustomobject]@{ RoundDef = $round; Protocol = 'IPv4'; Host = 'example.local' }) -PortTargets @([pscustomobject]@{ Name = 'Demo'; Protocol = 'UDP'; Port = 9999 }) -Settings ([pscustomobject]@{ PingCount = 5; SkipPathping = $true })

    $result.PathpingStatus | Should -Be 'Skipped'
    $result.PathpingOk | Should -Be $null
    $result.OverallStatus | Should -Be 'OK'
  }

  It 'fails overall status when non-TCP stages fail' {
    Mock Invoke-PingRaw { [pscustomobject]@{ Raw = @('ping'); ExitCode = 1 } }
    Mock Invoke-TracertRaw { [pscustomobject]@{ Raw = @('tracert'); ExitCode = 0 } }
    Mock Invoke-PathpingRaw { [pscustomobject]@{ Raw = @('pathping'); ExitCode = 0 } }
    Mock Test-TcpPort { [pscustomobject]@{ TcpTestSucceeded = $true; TraceRoute = @() } }
    Mock Test-UdpPortBestEffort { [pscustomobject]@{ Status = 'Likely reachable'; Detail = 'ok' } }

    $round = (Get-RoundDefinitions -TraceMaxHops 30 -TraceTimeoutMs 5000 -PathpingProbes 50 -PathpingTimeoutMs 3000 | Select-Object -First 1)
    $result = Invoke-HostDiagnostics -PlanItem ([pscustomobject]@{ RoundDef = $round; Protocol = 'IPv4'; Host = 'example.local' }) -PortTargets @([pscustomobject]@{ Name = 'Demo'; Protocol = 'UDP'; Port = 9999 }) -Settings ([pscustomobject]@{ PingCount = 5; SkipPathping = $false })

    $result.Tcp443OK | Should -BeTrue
    $result.PingStatus | Should -Be 'Fail'
    $result.OverallStatus | Should -Be 'Fail'
    $result.FailedStages | Should -Contain 'Ping'
  }

  It 'writes explicit stage status columns to CSV output' {
    $jsonPath = Join-Path $TestDrive 'result.json'
    $csvPath = Join-Path $TestDrive 'result.csv'

    Mock Write-Status {}

    Save-DiagnosticResults -Results @(
      [pscustomobject]@{
        Timestamp = '2026-01-01T00:00:00Z'
        Round = 'Standard'
        Protocol = 'IPv4'
        Host = 'example.local'
        PingStatus = 'OK'
        TracertStatus = 'OK'
        PathpingStatus = 'Skipped'
        Tcp443Status = 'OK'
        PortsStatus = 'Fail'
        OverallStatus = 'Fail'
        PingOk = $true
        TracertOk = $true
        PathpingOk = $null
        Tcp443OK = $true
      }
    ) -JsonPath $jsonPath -CsvPath $csvPath

    $csv = Get-Content -LiteralPath $csvPath -Raw
    $csv | Should -Match 'PathpingStatus'
    $csv | Should -Match 'OverallStatus'
    $csv | Should -Match 'Skipped'
  }
}
