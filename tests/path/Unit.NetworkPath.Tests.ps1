BeforeAll {
  . "$PSScriptRoot/../../src/powershell/path/lib-ps/Test-HostNameSafe.ps1"
  . "$PSScriptRoot/../../src/powershell/path/lib-ps/Test-PathSafe.ps1"
}

Describe 'Test-HostNameSafe' {
  It 'accepts valid hostname' {
    Test-HostNameSafe 'google.com' | Should -BeTrue
  }
  It 'accepts punycode IDN' {
    Test-HostNameSafe 'xn--nxasmq6b.com' | Should -BeTrue
  }
  It 'rejects empty string' {
    Test-HostNameSafe '' | Should -BeFalse
  }
  It 'rejects whitespace only' {
    Test-HostNameSafe '   ' | Should -BeFalse
  }
  It 'rejects dash prefix' {
    Test-HostNameSafe '-evil' | Should -BeFalse
  }
  It 'rejects whitespace in name' {
    Test-HostNameSafe 'host name' | Should -BeFalse
  }
  It 'rejects slash' {
    Test-HostNameSafe 'host/path' | Should -BeFalse
  }
  It 'rejects pipe' {
    Test-HostNameSafe 'host|cmd' | Should -BeFalse
  }
  It 'rejects semicolon' {
    Test-HostNameSafe 'host;cmd' | Should -BeFalse
  }
  It 'rejects ampersand' {
    Test-HostNameSafe 'host&cmd' | Should -BeFalse
  }
  It 'rejects backtick' {
    Test-HostNameSafe 'host`cmd' | Should -BeFalse
  }
  It 'rejects dollar sign' {
    Test-HostNameSafe 'host$var' | Should -BeFalse
  }
  It 'rejects spreadsheet formula prefixes' {
    Test-HostNameSafe '=1+1' | Should -BeFalse
    Test-HostNameSafe '+cmd' | Should -BeFalse
    Test-HostNameSafe '@SUM(1,1)' | Should -BeFalse
  }
  It 'rejects control character' {
    Test-HostNameSafe "host$([char]1)name" | Should -BeFalse
  }
}

Describe 'Test-PathSafe' {
  It 'accepts valid path' {
    Test-PathSafe '/tmp/logs' | Should -BeTrue
  }
  It 'accepts Windows path' {
    Test-PathSafe 'C:\Users\test\logs' | Should -BeTrue
  }
  It 'rejects empty string' {
    Test-PathSafe '' | Should -BeFalse
  }
  It 'rejects whitespace only' {
    Test-PathSafe '   ' | Should -BeFalse
  }
  It 'rejects dash prefix' {
    Test-PathSafe '-option' | Should -BeFalse
  }
  It 'rejects pipe' {
    Test-PathSafe 'foo|bar' | Should -BeFalse
  }
  It 'rejects control character' {
    Test-PathSafe "foo$([char]1)bar" | Should -BeFalse
  }
  It 'rejects path traversal /../' {
    Test-PathSafe '/foo/../bar' | Should -BeFalse
  }
  It 'rejects leading ../' {
    Test-PathSafe '../etc/passwd' | Should -BeFalse
  }
  It 'rejects bare ..' {
    Test-PathSafe '..' | Should -BeFalse
  }
}
