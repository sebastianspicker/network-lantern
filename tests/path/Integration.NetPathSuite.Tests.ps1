Describe 'NetPathSuite integration tests' {
  It 'DryRun exits without error' {
    $null = pwsh -NoProfile -NonInteractive -File "$PSScriptRoot/../../apps/path/NetPathSuite.ps1" -DryRun -Quiet 2>&1
    $LASTEXITCODE | Should -Be 0
  }

  It 'DryRun with IPv4 only' {
    $null = pwsh -NoProfile -NonInteractive -File "$PSScriptRoot/../../apps/path/NetPathSuite.ps1" -DryRun -Quiet -Protocols IPv4 2>&1
    $LASTEXITCODE | Should -Be 0
  }

  It 'DryRun with specific round' {
    $null = pwsh -NoProfile -NonInteractive -File "$PSScriptRoot/../../apps/path/NetPathSuite.ps1" -DryRun -Quiet -Rounds Standard 2>&1
    $LASTEXITCODE | Should -Be 0
  }

  It 'DryRun with custom host' {
    $null = pwsh -NoProfile -NonInteractive -File "$PSScriptRoot/../../apps/path/NetPathSuite.ps1" -DryRun -Quiet -HostsIPv4 localhost 2>&1
    $LASTEXITCODE | Should -Be 0
  }

  It 'DryRun output includes planned run count' {
    $output = pwsh -NoProfile -NonInteractive -File "$PSScriptRoot/../../apps/path/NetPathSuite.ps1" -DryRun 2>&1
    ($output | Out-String) | Should -Match 'Planned runs: \d+'
  }
}

Describe 'NetPathSuite new feature tests' {
  It '-Version prints version' {
    $output = pwsh -NoProfile -NonInteractive -File "$PSScriptRoot/../../apps/path/NetPathSuite.ps1" -Version 2>&1
    ($output | Out-String) | Should -Match 'v1\.1\.0'
  }

  It '-ListRounds prints round names' {
    $output = pwsh -NoProfile -NonInteractive -File "$PSScriptRoot/../../apps/path/NetPathSuite.ps1" -ListRounds 2>&1
    ($output | Out-String) | Should -Match 'Standard'
    ($output | Out-String) | Should -Match 'MTU1400_DF'
    ($output | Out-String) | Should -Match 'TTL64_Timeout5s'
  }

  It '-ListProtocols prints IPv4 and IPv6' {
    $output = pwsh -NoProfile -NonInteractive -File "$PSScriptRoot/../../apps/path/NetPathSuite.ps1" -ListProtocols 2>&1
    ($output | Out-String) | Should -Match 'IPv4'
    ($output | Out-String) | Should -Match 'IPv6'
  }
}
