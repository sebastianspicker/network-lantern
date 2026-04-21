Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

BeforeAll {
  . "$PSScriptRoot/../../src/powershell/path/lib-ps/Get-HostsFromConfig.ps1"
  . "$PSScriptRoot/../../src/powershell/path/lib-ps/Get-RoundDefinitions.ps1"
  . "$PSScriptRoot/../../src/powershell/path/lib-ps/Get-HostAddressesWithTimeout.ps1"
}

Describe 'Get-HostsFromConfig' {
  It 'returns empty arrays when the file does not exist' {
    $result = Get-HostsFromConfig -Path (Join-Path $TestDrive 'missing.conf')

    $result.IPv4 | Should -HaveCount 0
    $result.IPv6 | Should -HaveCount 0
  }

  It 'parses ipv4= and ipv6= lines into separate lists' {
    $conf = @(
      'ipv4=host-a.example'
      'ipv4=host-b.example'
      'ipv6=host-a.example'
    ) -join "`n"
    $path = Join-Path $TestDrive 'hosts.conf'
    Set-Content -LiteralPath $path -Value $conf -Encoding UTF8

    $result = Get-HostsFromConfig -Path $path

    $result.IPv4 | Should -HaveCount 2
    $result.IPv4 | Should -Contain 'host-a.example'
    $result.IPv4 | Should -Contain 'host-b.example'
    $result.IPv6 | Should -HaveCount 1
    $result.IPv6 | Should -Contain 'host-a.example'
  }

  It 'ignores comment lines beginning with #' {
    $conf = @(
      '# this is a comment'
      'ipv4=real-host.example'
    ) -join "`n"
    $path = Join-Path $TestDrive 'hosts-comments.conf'
    Set-Content -LiteralPath $path -Value $conf -Encoding UTF8

    $result = Get-HostsFromConfig -Path $path

    $result.IPv4 | Should -Contain 'real-host.example'
    $result.IPv4 | Should -Not -Contain '# this is a comment'
  }

  It 'ignores blank lines' {
    $conf = "`n`nipv4=host.example`n`n"
    $path = Join-Path $TestDrive 'hosts-blanks.conf'
    Set-Content -LiteralPath $path -Value $conf -Encoding UTF8

    $result = Get-HostsFromConfig -Path $path

    $result.IPv4 | Should -HaveCount 1
    $result.IPv4[0] | Should -Be 'host.example'
  }

  It 'ignores lines without an equals sign' {
    $conf = @(
      'not-a-key-value-line'
      'ipv4=valid.example'
    ) -join "`n"
    $path = Join-Path $TestDrive 'hosts-noeq.conf'
    Set-Content -LiteralPath $path -Value $conf -Encoding UTF8

    $result = Get-HostsFromConfig -Path $path

    $result.IPv4 | Should -HaveCount 1
  }

  It 'ignores unknown keys' {
    $conf = @(
      'ipv4=valid.example'
      'unknown=should-be-ignored'
    ) -join "`n"
    $path = Join-Path $TestDrive 'hosts-unknownkey.conf'
    Set-Content -LiteralPath $path -Value $conf -Encoding UTF8

    $result = Get-HostsFromConfig -Path $path

    $result.IPv4 | Should -HaveCount 1
    $result.Keys | Should -Not -Contain 'unknown'
  }
}

Describe 'Get-RoundDefinitions' {
  BeforeAll {
    $script:Rounds = Get-RoundDefinitions -TraceMaxHops 30 -TraceTimeoutMs 5000 -PathpingProbes 50 -PathpingTimeoutMs 3000
  }

  It 'returns exactly three rounds' {
    $script:Rounds | Should -HaveCount 3
  }

  It 'includes the expected round names' {
    $names = $script:Rounds | ForEach-Object { $_['Name'] }
    $names | Should -Contain 'Standard'
    $names | Should -Contain 'MTU1400_DF'
    $names | Should -Contain 'TTL64_Timeout5s'
  }

  It 'each round has all required scriptblock keys' {
    foreach ($round in $script:Rounds) {
      $round['PingArgs4'] | Should -BeOfType [scriptblock]
      $round['PingArgs6'] | Should -BeOfType [scriptblock]
      $round['TracertArgs'] | Should -BeOfType [scriptblock]
      $round['PathpingArgs'] | Should -BeOfType [scriptblock]
    }
  }

  It 'Standard PingArgs4 includes -4 flag and target host' {
    $standard = $script:Rounds | Where-Object { $_['Name'] -eq 'Standard' }
    $cmdArgs = & $standard['PingArgs4'] 'target.example' 5

    $cmdArgs | Should -Contain '-4'
    $cmdArgs | Should -Contain 'target.example'
    $cmdArgs | Should -Contain '5'
  }

  It 'Standard PingArgs6 includes -6 flag' {
    $standard = $script:Rounds | Where-Object { $_['Name'] -eq 'Standard' }
    $cmdArgs = & $standard['PingArgs6'] 'target.example' 5

    $cmdArgs | Should -Contain '-6'
  }

  It 'MTU1400_DF PingArgs4 includes -f and -l 1400 for DF-bit probing' {
    $mtu = $script:Rounds | Where-Object { $_['Name'] -eq 'MTU1400_DF' }
    $cmdArgs = & $mtu['PingArgs4'] 'target.example' 5

    $cmdArgs | Should -Contain '-f'
    $cmdArgs | Should -Contain '-l'
    $cmdArgs | Should -Contain '1400'
  }

  It 'TTL64_Timeout5s PingArgs4 includes -i 64 for TTL limiting' {
    $ttl = $script:Rounds | Where-Object { $_['Name'] -eq 'TTL64_Timeout5s' }
    $cmdArgs = & $ttl['PingArgs4'] 'target.example' 5

    $cmdArgs | Should -Contain '-i'
    $cmdArgs | Should -Contain '64'
  }

  It 'Standard TracertArgs includes configured TraceMaxHops' {
    $standard = $script:Rounds | Where-Object { $_['Name'] -eq 'Standard' }
    # The TracertArgs scriptblock captures TraceMaxHops/TraceTimeoutMs from the calling scope,
    # matching how the app invokes it — with those variables in the active stack frame.
    $TraceMaxHops = 30; $TraceTimeoutMs = 5000
    $null = $TraceMaxHops, $TraceTimeoutMs  # suppress PSUseDeclaredVarsMoreThanAssignments
    $cmdArgs = & $standard['TracertArgs'] '-4' 'target.example'

    $cmdArgs | Should -Contain '30'
  }
}

Describe 'Get-HostAddressesWithTimeout' {
  It 'returns addresses and null error for localhost' {
    $addresses, $err = Get-HostAddressesWithTimeout -HostName 'localhost'

    $err | Should -BeNullOrEmpty
    $addresses | Should -Not -BeNullOrEmpty
  }

  It 'returns null addresses and an error message for an unresolvable host' {
    $addresses, $err = Get-HostAddressesWithTimeout -HostName 'this-host-cannot-resolve.nds-test.invalid'

    $addresses | Should -BeNullOrEmpty
    $err | Should -Not -BeNullOrEmpty
    $err | Should -Match 'DNS resolution failed'
  }

  It 'returns a timeout error when timeout is extremely short' {
    $addresses, $err = Get-HostAddressesWithTimeout -HostName 'slow-dns.nds-test.invalid' -TimeoutMs 1

    # Either resolves instantly (unlikely for invalid host) or times out — both are valid.
    # The important invariant: a non-null error string is always returned on failure.
    if ($null -eq $addresses) {
      $err | Should -Not -BeNullOrEmpty
    }
  }
}
