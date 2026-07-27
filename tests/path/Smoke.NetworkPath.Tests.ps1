Describe 'Network path smoke tests' {
  It 'Script file exists' {
    Test-Path "$PSScriptRoot/../../apps/path/Test-NetworkPath.ps1" | Should -BeTrue
  }

  It 'Config file exists' {
    Test-Path "$PSScriptRoot/../../config/hosts.conf" | Should -BeTrue
  }
}
