Describe 'NetPathSuite smoke tests' {
  It 'Script file exists' {
    Test-Path "$PSScriptRoot/../../apps/path/NetPathSuite.ps1" | Should -BeTrue
  }

  It 'Config file exists' {
    Test-Path "$PSScriptRoot/../../config/hosts.conf" | Should -BeTrue
  }
}
