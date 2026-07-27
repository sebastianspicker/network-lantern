Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Describe 'Invoke-NetworkLantern orchestration' {
BeforeAll {
    $script:InvokeNetworkLanternScriptPath = Join-Path $PSScriptRoot '../../Invoke-NetworkLantern.ps1'
    $script:IsWindowsRuntime = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
      [System.Runtime.InteropServices.OSPlatform]::Windows
    )
  }

  It 'preserves configured triage path arguments in dry-run mode' {
    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkLanternScriptPath -Workflow Triage -DryRun 2>&1

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
& '$script:InvokeNetworkLanternScriptPath' @params
"@
    $output = & pwsh -NoLogo -NoProfile -NonInteractive -Command $command 2>&1

    $LASTEXITCODE | Should -Be 0
    ($output | Out-String) | Should -Match 'Protocols:\s+IPv6'
    ($output | Out-String) | Should -Match 'Rounds:\s+Standard, TTL64_Timeout5s'
    ($output | Out-String) | Should -Match 'IPv4 hosts:\s+ipv4-a\.example, ipv4-b\.example'
    ($output | Out-String) | Should -Match 'IPv6 hosts:\s+ipv6-a\.example, ipv6-b\.example'
  }

  It 'runs baseline as path plus throughput in dry-run mode' {
    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkLanternScriptPath -Workflow Baseline -IperfTarget iperf3.example.net -DryRun 2>&1

    $LASTEXITCODE | Should -Be 0
    ($output | Out-String) | Should -Match 'Dry-run only\. Planned runs:'
    ($output | Out-String) | Should -Match 'WhatIf: Would run approximately'
    ($output | Out-String) | Should -Match 'Target:\s+iperf3\.example\.net'
  }

  It 'runs WindowsTuning Verify workflow without requiring administrator' {
    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkLanternScriptPath `
      -Workflow WindowsTuning -TuningAction Verify -DryRun 2>&1

    if (-not $script:IsWindowsRuntime) {
      $LASTEXITCODE | Should -Not -Be 0
    }
    ($output | Out-String) | Should -Not -Match 'Please run as Administrator'
  }

  It 'runs WindowsTuning Apply dry-run without requiring administrator' {
    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkLanternScriptPath `
      -Workflow WindowsTuning -TuningAction Apply -TuningProfile Safe -DryRun 2>&1

    $LASTEXITCODE | Should -Be 0
    ($output | Out-String) | Should -Not -Match 'Please run as Administrator'
  }

  It 'fails with a clear error when IperfTarget is absent for Throughput workflow' {
    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkLanternScriptPath `
      -Workflow Throughput 2>&1

    $LASTEXITCODE | Should -Not -Be 0
    ($output | Out-String) | Should -Match 'IperfTarget is required'
  }

  It 'propagates a failed child workflow exit status' {
    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkLanternScriptPath `
      -Workflow Path -HostsIPv4 'invalid host name' -DryRun 2>&1

    $LASTEXITCODE | Should -Not -Be 0
    ($output | Out-String) | Should -Match 'Host names must not start with'
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

    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkLanternScriptPath `
      -Workflow Path -ProfilePath $profileFile -DryRun 2>&1

    $LASTEXITCODE | Should -Be 0
    ($output | Out-String) | Should -Match 'profile-host\.example'
    ($output | Out-String) | Should -Match 'Protocols:\s+IPv4'
  }

  It 'loads profile throughput values when matching CLI arguments are omitted' {
    $profileContent = @{
      throughput = @{
        target = 'profile-iperf.example'
        port = 5202
        protocol = 'UDP'
      }
    } | ConvertTo-Json -Depth 4

    $profileFile = Join-Path $TestDrive 'throughput-profile.json'
    Set-Content -LiteralPath $profileFile -Value $profileContent -Encoding UTF8

    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkLanternScriptPath `
      -Workflow Throughput -ProfilePath $profileFile -DryRun 2>&1

    $LASTEXITCODE | Should -Be 0
    ($output | Out-String) | Should -Match 'Target:\s+profile-iperf\.example'
    ($output | Out-String) | Should -Match 'Port:\s+5202'
    ($output | Out-String) | Should -Match 'WhatIf: Would run approximately \d+ tests\.'
    ($output | Out-String) | Should -Match 'Protocol:\s+UDP'
  }

  It 'keeps explicit throughput CLI arguments ahead of profile values' {
    $profileContent = @{
      throughput = @{
        target = 'profile-iperf.example'
        port = 5202
        protocol = 'UDP'
      }
    } | ConvertTo-Json -Depth 4

    $profileFile = Join-Path $TestDrive 'throughput-precedence-profile.json'
    Set-Content -LiteralPath $profileFile -Value $profileContent -Encoding UTF8

    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkLanternScriptPath `
      -Workflow Throughput -ProfilePath $profileFile -IperfTarget cli-iperf.example -IperfPort 5203 -ThroughputProtocol TCP -DryRun 2>&1

    $LASTEXITCODE | Should -Be 0
    ($output | Out-String) | Should -Match 'Target:\s+cli-iperf\.example'
    ($output | Out-String) | Should -Match 'Port:\s+5203'
    ($output | Out-String) | Should -Match 'WhatIf: Would run approximately \d+ tests\.'
    ($output | Out-String) | Should -Match 'Protocol:\s+TCP'
  }

  It 'loads profile Windows tuning action and profile when matching CLI arguments are omitted' {
    $profileContent = @{
      windowsTuning = @{
        action = 'Verify'
        profile = 'Measured'
        udpPorts = @(5202)
      }
    } | ConvertTo-Json -Depth 4

    $profileFile = Join-Path $TestDrive 'tuning-profile.json'
    Set-Content -LiteralPath $profileFile -Value $profileContent -Encoding UTF8

    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkLanternScriptPath `
      -Workflow WindowsTuning -ProfilePath $profileFile -DryRun 2>&1

    if (-not $script:IsWindowsRuntime) {
      $LASTEXITCODE | Should -Not -Be 0
    }
    ($output | Out-String) | Should -Not -Match 'Please run as Administrator'
  }

  It 'keeps explicit Windows tuning CLI arguments ahead of profile values' {
    $profileContent = @{
      windowsTuning = @{
        action = 'Apply'
        profile = 'Measured'
        udpPorts = @(5202)
      }
    } | ConvertTo-Json -Depth 4

    $profileFile = Join-Path $TestDrive 'tuning-precedence-profile.json'
    Set-Content -LiteralPath $profileFile -Value $profileContent -Encoding UTF8

    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkLanternScriptPath `
      -Workflow WindowsTuning -ProfilePath $profileFile -TuningAction Verify -TuningProfile Safe -UdpPorts 5203 -DryRun 2>&1

    if (-not $script:IsWindowsRuntime) {
      $LASTEXITCODE | Should -Not -Be 0
    }
    ($output | Out-String) | Should -Not -Match 'Please run as Administrator'
  }

  It 'warns about unknown workflow profile sections and keys' {
    $profileContent = @{
      path = @{
        hostsIPv4 = @('profile-host.example')
        typoHosts = @('ignored.example')
      }
      throughputTypo = @{
        target = 'ignored.example'
      }
    } | ConvertTo-Json -Depth 4

    $profileFile = Join-Path $TestDrive 'unknown-profile-keys.json'
    Set-Content -LiteralPath $profileFile -Value $profileContent -Encoding UTF8

    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkLanternScriptPath `
      -Workflow Path -ProfilePath $profileFile -DryRun 2>&1

    $LASTEXITCODE | Should -Be 0
    ($output | Out-String) | Should -Match "Unknown workflow profile key 'path\.typoHosts'"
    ($output | Out-String) | Should -Match "Unknown workflow profile section 'throughputTypo'"
    ($output | Out-String) | Should -Match 'profile-host\.example'
  }

  It 'does not warn for known workflow profile sections and keys' {
    $profileContent = @{
      path = @{
        hostsIPv4 = @('profile-host.example')
        hostsIPv6 = @('profile-v6.example')
        protocols = @('IPv4')
        rounds = @('Standard')
      }
      throughput = @{
        target = 'profile-iperf.example'
        port = 5202
        protocol = 'UDP'
      }
      windowsTuning = @{
        action = 'Verify'
        profile = 'Measured'
        udpPorts = @(5202)
        appPaths = @('C:\Tools\App.exe')
      }
    } | ConvertTo-Json -Depth 4

    $profileFile = Join-Path $TestDrive 'known-profile-keys.json'
    Set-Content -LiteralPath $profileFile -Value $profileContent -Encoding UTF8

    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkLanternScriptPath `
      -Workflow Path -ProfilePath $profileFile -DryRun 2>&1

    $LASTEXITCODE | Should -Be 0
    ($output | Out-String) | Should -Not -Match 'Unknown workflow profile'
  }

  It 'rejects oversized workflow profiles before parsing JSON' {
    $profileFile = Join-Path $TestDrive 'oversized-profile.json'
    Set-Content -LiteralPath $profileFile -Value ('x' * (1MB + 1)) -NoNewline -Encoding UTF8

    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkLanternScriptPath `
      -Workflow Path -ProfilePath $profileFile -DryRun 2>&1

    $LASTEXITCODE | Should -Not -Be 0
    ($output | Out-String) | Should -Match 'ProfilePath exceeds maximum size'
  }

  It 'Triage reports skipped throughput when IperfTarget is not provided' {
    $output = & pwsh -NoLogo -NoProfile -NonInteractive -File $script:InvokeNetworkLanternScriptPath `
      -Workflow Triage -DryRun 2>&1

    $LASTEXITCODE | Should -Be 0
    ($output | Out-String) | Should -Match 'Planned runs:'
    ($output | Out-String) | Should -Not -Match 'WhatIf: Would run approximately'
    ($output | Out-String) | Should -Not -Match 'IperfTarget is required'
    ($output | Out-String) | Should -Match 'Throughput skipped: IperfTarget not provided'
  }
}
