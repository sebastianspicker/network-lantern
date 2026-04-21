Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Describe 'Invoke-NetworkDiagnostics orchestration' {
  BeforeAll {
    $script:InvokeNetworkDiagnosticsScriptPath = Join-Path $PSScriptRoot '../../Invoke-NetworkDiagnostics.ps1'
  }

  It 'preserves configured triage path arguments in dry-run mode' {
    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkDiagnosticsScriptPath -Workflow Triage -DryRun 2>&1

    $LASTEXITCODE | Should -Be 0
    ($output | Out-String) | Should -Match 'Protocols:\s+IPv4, IPv6'
    ($output | Out-String) | Should -Match 'IPv4 hosts:\s+cloudflare\.com, google\.com, wikipedia\.org, amazon\.de'
    ($output | Out-String) | Should -Match 'IPv6 hosts:\s+cloudflare\.com, google\.com, wikipedia\.org'
    ($output | Out-String) | Should -Not -Match 'IPv4 hosts:\s+IPv6'
  }

  It 'preserves explicit multi-value path arguments in dry-run mode' {
    $command = @"
`$params = @{
  Workflow = 'Path'
  DryRun = `$true
  Protocols = @('IPv6')
  Rounds = @('Standard', 'TTL64_Timeout5s')
  HostsIPv4 = @('ipv4-a.example', 'ipv4-b.example')
  HostsIPv6 = @('ipv6-a.example', 'ipv6-b.example')
}
& '$script:InvokeNetworkDiagnosticsScriptPath' @params
"@
    $output = & pwsh -NoLogo -NoProfile -NonInteractive -Command $command 2>&1

    $LASTEXITCODE | Should -Be 0
    ($output | Out-String) | Should -Match 'Protocols:\s+IPv6'
    ($output | Out-String) | Should -Match 'Rounds:\s+Standard, TTL64_Timeout5s'
    ($output | Out-String) | Should -Match 'IPv4 hosts:\s+ipv4-a\.example, ipv4-b\.example'
    ($output | Out-String) | Should -Match 'IPv6 hosts:\s+ipv6-a\.example, ipv6-b\.example'
  }

  It 'runs baseline as path plus throughput in dry-run mode' {
    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkDiagnosticsScriptPath -Workflow Baseline -IperfTarget iperf3.example.net -DryRun 2>&1

    $LASTEXITCODE | Should -Be 0
    ($output | Out-String) | Should -Match 'Dry-run only\. Planned runs:'
    ($output | Out-String) | Should -Match 'WhatIf: Would run approximately'
    ($output | Out-String) | Should -Match 'Target:\s+iperf3\.example\.net'
  }

  It 'runs WindowsTuning Verify workflow without requiring administrator' {
    $null = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkDiagnosticsScriptPath `
      -Workflow WindowsTuning -TuningAction Verify -DryRun 2>&1

    $LASTEXITCODE | Should -Be 0
  }

  It 'fails with a clear error when IperfTarget is absent for Throughput workflow' {
    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkDiagnosticsScriptPath `
      -Workflow Throughput 2>&1

    $LASTEXITCODE | Should -Not -Be 0
    ($output | Out-String) | Should -Match 'IperfTarget is required'
  }

  It 'loads profile values via -ProfilePath and uses them in path dry-run' {
    $profileContent = @{
      path = @{
        hostsIPv4 = @('profile-host.example')
        protocols = @('IPv4')
      }
    } | ConvertTo-Json -Depth 4

    $profileFile = Join-Path $TestDrive 'test-profile.json'
    Set-Content -LiteralPath $profileFile -Value $profileContent -Encoding UTF8

    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkDiagnosticsScriptPath `
      -Workflow Path -ProfilePath $profileFile -DryRun 2>&1

    $LASTEXITCODE | Should -Be 0
    ($output | Out-String) | Should -Match 'profile-host\.example'
    ($output | Out-String) | Should -Match 'Protocols:\s+IPv4'
  }

  It 'Triage skips throughput silently when IperfTarget is not provided' {
    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkDiagnosticsScriptPath `
      -Workflow Triage -DryRun 2>&1

    $LASTEXITCODE | Should -Be 0
    ($output | Out-String) | Should -Match 'Planned runs:'
    ($output | Out-String) | Should -Not -Match 'WhatIf: Would run approximately'
    ($output | Out-String) | Should -Not -Match 'IperfTarget is required'
  }
}
