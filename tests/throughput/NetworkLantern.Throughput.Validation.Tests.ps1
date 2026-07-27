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
  Context 'DSCP mapping' {
    It 'maps CS0 to 0' {
      InModuleScope 'NetworkLantern.Throughput' {
        Get-TosFromDscpClass -Class 'CS0' | Should -Be 0
      }
    }

    It 'maps EF to 184' {
      InModuleScope 'NetworkLantern.Throughput' {
        Get-TosFromDscpClass -Class 'EF' | Should -Be (46 -shl 2)
      }
    }

    It 'maps AF11 to 40' {
      InModuleScope 'NetworkLantern.Throughput' {
        Get-TosFromDscpClass -Class 'AF11' | Should -Be 40
      }
    }

    It 'throws on unknown DSCP class' {
      InModuleScope 'NetworkLantern.Throughput' {
        { Get-TosFromDscpClass -Class 'NOPE' } | Should -Throw "Unknown DSCP class*"
      }
    }
  }

  Context 'Hostname/IP validation' {
    It 'rejects invalid IPv6-like strings (e.g. :::)' {
      InModuleScope 'NetworkLantern.Throughput' {
        Test-ValidHostnameOrIP -Name ':::' | Should -Be $false
        Test-ValidHostnameOrIP -Name ':' | Should -Be $false
      }
    }
  }

  Context 'Bandwidth parsing' {
    It 'parses 10M to 10' {
      InModuleScope 'NetworkLantern.Throughput' {
        ConvertTo-MbitPerSecond -Value '10M' | Should -Be 10.0
      }
    }

    It 'parses 1G to 1000' {
      InModuleScope 'NetworkLantern.Throughput' {
        ConvertTo-MbitPerSecond -Value '1G' | Should -Be 1000.0
      }
    }

    It 'parses 500K to 0.5' {
      InModuleScope 'NetworkLantern.Throughput' {
        ConvertTo-MbitPerSecond -Value '500K' | Should -Be 0.5
      }
    }

    It 'throws on invalid format' {
      InModuleScope 'NetworkLantern.Throughput' {
        { ConvertTo-MbitPerSecond -Value 'nope' } | Should -Throw
      }
    }
  }

  Context 'Configuration normalization' {
    It 'ignores unknown keys in non-strict mode' {
      InModuleScope 'NetworkLantern.Throughput' {
        $res = ConvertTo-Iperf3NormalizedParameterSet -InputParameters @{ Port = 5201; UnknownKey = 'x' } -AllowedKeys @('Port') -StrictConfiguration:$false
        $res.Parameters.ContainsKey('Port') | Should -Be $true
        $res.Parameters.ContainsKey('UnknownKey') | Should -Be $false
        @($res.Warnings).Count | Should -Be 1
      }
    }

    It 'throws on unknown keys in strict mode' {
      InModuleScope 'NetworkLantern.Throughput' {
        { ConvertTo-Iperf3NormalizedParameterSet -InputParameters @{ UnknownKey = 'x' } -AllowedKeys @('Port') -StrictConfiguration } | Should -Throw
      }
    }

    It 'drops invalid range values in non-strict mode' {
      InModuleScope 'NetworkLantern.Throughput' {
        $res = ConvertTo-Iperf3NormalizedParameterSet -InputParameters @{ Port = 70000 } -AllowedKeys @('Port') -StrictConfiguration:$false
        $res.Parameters.ContainsKey('Port') | Should -Be $false
        @($res.Warnings).Count | Should -Be 1
        $res.Warnings[0] | Should -Match 'range'
      }
    }

    It 'throws on invalid range values in strict mode' {
      InModuleScope 'NetworkLantern.Throughput' {
        { ConvertTo-Iperf3NormalizedParameterSet -InputParameters @{ Port = 70000 } -AllowedKeys @('Port') -StrictConfiguration } | Should -Throw
      }
    }
  }

  Context 'Hostname/IP validation edge cases' {
    It 'accepts valid hostnames' {
      InModuleScope 'NetworkLantern.Throughput' {
        Test-ValidHostnameOrIP -Name 'example.com' | Should -Be $true
        Test-ValidHostnameOrIP -Name 'my-host' | Should -Be $true
        Test-ValidHostnameOrIP -Name 'a.b.c.d.e' | Should -Be $true
      }
    }

    It 'accepts valid IPv4 addresses' {
      InModuleScope 'NetworkLantern.Throughput' {
        Test-ValidHostnameOrIP -Name '192.168.1.1' | Should -Be $true
        Test-ValidHostnameOrIP -Name '0.0.0.0' | Should -Be $true
        Test-ValidHostnameOrIP -Name '255.255.255.255' | Should -Be $true
      }
    }

    It 'rejects IPv4 octets out of range' {
      InModuleScope 'NetworkLantern.Throughput' {
        Test-ValidHostnameOrIP -Name '256.1.1.1' | Should -Be $false
        Test-ValidHostnameOrIP -Name '1.1.1.999' | Should -Be $false
      }
    }

    It 'rejects names starting with dash (argument injection)' {
      InModuleScope 'NetworkLantern.Throughput' {
        Test-ValidHostnameOrIP -Name '-evil' | Should -Be $false
        Test-ValidHostnameOrIP -Name '--version' | Should -Be $false
      }
    }

    It 'rejects empty and whitespace' {
      InModuleScope 'NetworkLantern.Throughput' {
        # Empty string rejected at parameter binding (Mandatory); whitespace returns $false
        { Test-ValidHostnameOrIP -Name '' } | Should -Throw
        Test-ValidHostnameOrIP -Name '   ' | Should -Be $false
      }
    }

    It 'accepts valid IPv6 addresses' {
      InModuleScope 'NetworkLantern.Throughput' {
        Test-ValidHostnameOrIP -Name '::1' | Should -Be $true
        Test-ValidHostnameOrIP -Name 'fe80::1' | Should -Be $true
        Test-ValidHostnameOrIP -Name '[::1]' | Should -Be $true
      }
    }

    It 'rejects malformed or scoped IPv6 literals' {
      InModuleScope 'NetworkLantern.Throughput' {
        Test-ValidHostnameOrIP -Name '2001:::1' | Should -Be $false
        Test-ValidHostnameOrIP -Name 'fe80::1%en0' | Should -Be $false
        Test-ValidHostnameOrIP -Name '[fe80::1%12]' | Should -Be $false
        Test-ValidHostnameOrIP -Name '[::1' | Should -Be $false
      }
    }
  }

  Context 'DSCP mapping edge cases' {
    It 'maps all CS classes correctly' {
      InModuleScope 'NetworkLantern.Throughput' {
        Get-TosFromDscpClass -Class 'CS0' | Should -Be 0
        Get-TosFromDscpClass -Class 'CS1' | Should -Be 32
        Get-TosFromDscpClass -Class 'CS7' | Should -Be 224
      }
    }

    It 'maps AF classes correctly' {
      InModuleScope 'NetworkLantern.Throughput' {
        Get-TosFromDscpClass -Class 'AF41' | Should -Be 136
        Get-TosFromDscpClass -Class 'AF43' | Should -Be 152
      }
    }

    It 'throws on invalid DSCP like CS8 or AF14' {
      InModuleScope 'NetworkLantern.Throughput' {
        { Get-TosFromDscpClass -Class 'CS8' } | Should -Throw
        { Get-TosFromDscpClass -Class 'AF14' } | Should -Throw
        { Get-TosFromDscpClass -Class '' } | Should -Throw
      }
    }
  }

  Context 'Bandwidth conversion edge cases' {
    It 'treats bare numbers as megabits' {
      InModuleScope 'NetworkLantern.Throughput' {
        ConvertTo-MbitPerSecond -Value '100' | Should -Be 100.0
      }
    }

    It 'handles lowercase and uppercase suffixes' {
      InModuleScope 'NetworkLantern.Throughput' {
        ConvertTo-MbitPerSecond -Value '10m' | Should -Be 10.0
        ConvertTo-MbitPerSecond -Value '10M' | Should -Be 10.0
        ConvertTo-MbitPerSecond -Value '1g' | Should -Be 1000.0
        ConvertTo-MbitPerSecond -Value '1G' | Should -Be 1000.0
      }
    }

    It 'throws on empty/whitespace input' {
      InModuleScope 'NetworkLantern.Throughput' {
        # Empty string rejected at parameter binding (Mandatory); whitespace throws explicitly
        { ConvertTo-MbitPerSecond -Value '' } | Should -Throw
        { ConvertTo-MbitPerSecond -Value '  ' } | Should -Throw
      }
    }
  }

  Context 'DSCP class validation in config' {
    It 'accepts valid DSCP class names' {
      InModuleScope 'NetworkLantern.Throughput' {
        $result = ConvertTo-Iperf3KnownValue -Key 'DscpClasses' -Value @('CS0', 'EF', 'AF11', 'AF43')
        $result | Should -Contain 'CS0'
        $result | Should -Contain 'EF'
      }
    }

    It 'rejects invalid DSCP class names' {
      InModuleScope 'NetworkLantern.Throughput' {
        { ConvertTo-Iperf3KnownValue -Key 'DscpClasses' -Value @('CS8') } | Should -Throw '*Invalid DSCP class*'
        { ConvertTo-Iperf3KnownValue -Key 'DscpClasses' -Value @('AF14') } | Should -Throw '*Invalid DSCP class*'
        { ConvertTo-Iperf3KnownValue -Key 'DscpClasses' -Value @('NOPE') } | Should -Throw '*Invalid DSCP class*'
        { ConvertTo-Iperf3KnownValue -Key 'DscpClasses' -Value @('ef') } | Should -Throw '*Invalid DSCP class*'
      }
    }
  }

  Context 'TCP window size validation in config' {
    It 'accepts valid TCP window sizes' {
      InModuleScope 'NetworkLantern.Throughput' {
        $result = ConvertTo-Iperf3KnownValue -Key 'TcpWindows' -Value @('default', '128K', '256M', '1G', '4096')
        $result | Should -Contain 'default'
        $result | Should -Contain '128K'
      }
    }

    It 'rejects invalid TCP window sizes' {
      InModuleScope 'NetworkLantern.Throughput' {
        { ConvertTo-Iperf3KnownValue -Key 'TcpWindows' -Value @('abc') } | Should -Throw '*Invalid TCP window size*'
        { ConvertTo-Iperf3KnownValue -Key 'TcpWindows' -Value @('128T') } | Should -Throw '*Invalid TCP window size*'
        { ConvertTo-Iperf3KnownValue -Key 'TcpWindows' -Value @('-1K') } | Should -Throw '*Invalid TCP window size*'
      }
    }
  }

  Context 'OutDir config validation' {
    It 'rejects OutDir with control characters' {
      InModuleScope 'NetworkLantern.Throughput' {
        { ConvertTo-Iperf3KnownValue -Key 'OutDir' -Value "logs`0evil" } | Should -Throw '*control characters*'
        { ConvertTo-Iperf3KnownValue -Key 'OutDir' -Value "logs`ttab" } | Should -Throw '*control characters*'
      }
    }

    It 'rejects OutDir values that look like opener options' {
      InModuleScope 'NetworkLantern.Throughput' {
        { ConvertTo-Iperf3KnownValue -Key 'OutDir' -Value '-reports' } | Should -Throw '*must not look like an option*'
      }
    }

    It 'accepts normal OutDir paths' {
      InModuleScope 'NetworkLantern.Throughput' {
        $result = ConvertTo-Iperf3KnownValue -Key 'OutDir' -Value 'logs/output'
        $result | Should -Be 'logs/output'
      }
    }
  }

  Context 'Unknown parameter key error message' {
    It 'suggests Get-NetworkThroughputDefaultParameterSet' {
      InModuleScope 'NetworkLantern.Throughput' {
        { ConvertTo-Iperf3KnownValue -Key 'Bogus' -Value 'x' } | Should -Throw '*Get-NetworkThroughputDefaultParameterSet*'
      }
    }
  }

  Context 'Profile name validation' {
    It 'rejects profile names with invalid characters' {
      InModuleScope 'NetworkLantern.Throughput' {
        $profilesFile = Join-Path $TestDrive 'profiles-chartest.json'
        { Save-Iperf3Profile -ProfileName 'a/b' -ProfilesFile $profilesFile -Parameters @{ Target = 'x' } } | Should -Throw '*invalid characters*'
        { Save-Iperf3Profile -ProfileName 'a*b' -ProfilesFile $profilesFile -Parameters @{ Target = 'x' } } | Should -Throw '*invalid characters*'
        { Save-Iperf3Profile -ProfileName "a`0b" -ProfilesFile $profilesFile -Parameters @{ Target = 'x' } } | Should -Throw '*invalid characters*'
      }
    }

    It 'rejects profile names exceeding 128 characters' {
      InModuleScope 'NetworkLantern.Throughput' {
        $profilesFile = Join-Path $TestDrive 'profiles-lentest.json'
        $longName = 'x' * 129
        { Save-Iperf3Profile -ProfileName $longName -ProfilesFile $profilesFile -Parameters @{ Target = 'x' } } | Should -Throw '*exceeds maximum length*'
      }
    }

    It 'accepts valid profile names up to 128 characters' {
      InModuleScope 'NetworkLantern.Throughput' {
        $profilesFile = Join-Path $TestDrive 'profiles-validname.json'
        $okName = 'x' * 128
        $result = Save-Iperf3Profile -ProfileName $okName -ProfilesFile $profilesFile -Parameters @{ Target = 'example.local' }
        $result.ProfileName | Should -Be $okName
      }
    }
  }

  Context 'ProfilesFile path validation' {
    It 'rejects ProfilesFile with control characters' {
      InModuleScope 'NetworkLantern.Throughput' {
        { Resolve-ProfilesFilePath -ProfilesFile "test`0file.json" } | Should -Throw '*control characters*'
      }
    }

    It 'requires .json extension for writing' {
      InModuleScope 'NetworkLantern.Throughput' {
        $noJsonExt = Join-Path $TestDrive 'profiles.txt'
        { Save-Iperf3Profile -ProfileName 'lab' -ProfilesFile $noJsonExt -Parameters @{ Target = 'example.local' } } | Should -Throw '*.json extension*'
      }
    }

    It 'rejects profiles file exceeding 1 MB' {
      InModuleScope 'NetworkLantern.Throughput' {
        $bigFile = Join-Path $TestDrive 'big-profiles.json'
        $bigContent = '{"version":1,"profiles":{' + ('"k":' + ('"' + ('x' * 1000) + '",' ) * 1050) + '"end":"v"}}'
        Set-Content -LiteralPath $bigFile -Value $bigContent -Encoding UTF8
        { Read-Iperf3ProfilesStore -ProfilesFile $bigFile -StrictConfiguration } | Should -Throw '*exceeds maximum size*'
      }
    }
  }

  Context 'Error classification for new validation messages' {
    It 'classifies "invalid characters" as InputValidation' {
      InModuleScope 'NetworkLantern.Throughput' {
        $ex = New-Object System.Exception("ProfileName contains invalid characters: 'a/b'.")
        $er = New-Object System.Management.Automation.ErrorRecord($ex, 'test', 'NotSpecified', $null)
        $result = Resolve-Iperf3ClassifiedError -ErrorRecord $er
        $result.ErrorId | Should -Be 'NetworkLantern.Throughput.InputValidation'
      }
    }

    It 'classifies "exceeds maximum length" as InputValidation' {
      InModuleScope 'NetworkLantern.Throughput' {
        $ex = New-Object System.Exception("ProfileName exceeds maximum length (128 characters).")
        $er = New-Object System.Management.Automation.ErrorRecord($ex, 'test', 'NotSpecified', $null)
        $result = Resolve-Iperf3ClassifiedError -ErrorRecord $er
        $result.ErrorId | Should -Be 'NetworkLantern.Throughput.InputValidation'
      }
    }

    It 'classifies "Invalid DSCP class" as InputValidation' {
      InModuleScope 'NetworkLantern.Throughput' {
        $ex = New-Object System.Exception("Invalid DSCP class 'CS8' in 'DscpClasses'.")
        $er = New-Object System.Management.Automation.ErrorRecord($ex, 'test', 'NotSpecified', $null)
        $result = Resolve-Iperf3ClassifiedError -ErrorRecord $er
        $result.ErrorId | Should -Be 'NetworkLantern.Throughput.InputValidation'
      }
    }

    It 'classifies "Profiles file exceeds maximum size" as Prerequisite' {
      InModuleScope 'NetworkLantern.Throughput' {
        $ex = New-Object System.Exception("Profiles file exceeds maximum size (1 MB): /path/to/file")
        $er = New-Object System.Management.Automation.ErrorRecord($ex, 'test', 'NotSpecified', $null)
        $result = Resolve-Iperf3ClassifiedError -ErrorRecord $er
        $result.ErrorId | Should -Be 'NetworkLantern.Throughput.Prerequisite'
      }
    }

    It 'classifies ".json extension" as InputValidation' {
      InModuleScope 'NetworkLantern.Throughput' {
        $ex = New-Object System.Exception("Profiles file must have a .json extension: /path/to/file.txt")
        $er = New-Object System.Management.Automation.ErrorRecord($ex, 'test', 'NotSpecified', $null)
        $result = Resolve-Iperf3ClassifiedError -ErrorRecord $er
        $result.ErrorId | Should -Be 'NetworkLantern.Throughput.InputValidation'
      }
    }

    It 'classifies "control characters" as InputValidation' {
      InModuleScope 'NetworkLantern.Throughput' {
        $ex = New-Object System.Exception("OutDir path contains control characters.")
        $er = New-Object System.Management.Automation.ErrorRecord($ex, 'test', 'NotSpecified', $null)
        $result = Resolve-Iperf3ClassifiedError -ErrorRecord $er
        $result.ErrorId | Should -Be 'NetworkLantern.Throughput.InputValidation'
      }
    }

    It 'classifies option-like paths as InputValidation' {
      InModuleScope 'NetworkLantern.Throughput' {
        $ex = New-Object System.Exception("OutDir path must not look like an option (starts with '-'): -reports")
        $er = New-Object System.Management.Automation.ErrorRecord($ex, 'test', 'NotSpecified', $null)
        $result = Resolve-Iperf3ClassifiedError -ErrorRecord $er
        $result.ErrorId | Should -Be 'NetworkLantern.Throughput.InputValidation'
      }
    }

    It 'classifies unmatched patterns as Internal with verbose output' {
      InModuleScope 'NetworkLantern.Throughput' {
        $ex = New-Object System.Exception("Something completely unexpected happened.")
        $er = New-Object System.Management.Automation.ErrorRecord($ex, 'test', 'NotSpecified', $null)
        $result = Resolve-Iperf3ClassifiedError -ErrorRecord $er
        $result.ErrorId | Should -Be 'NetworkLantern.Throughput.Internal'
      }
    }
  }

  Context 'Test-PathUnderBase' {
    It 'accepts paths under the base' {
      InModuleScope 'NetworkLantern.Throughput' {
        $base = $TestDrive
        $child = Join-Path $TestDrive 'sub' 'file.txt'
        Test-PathUnderBase -BasePath $base -CandidatePath $child | Should -Be $true
      }
    }

    It 'accepts the base path itself' {
      InModuleScope 'NetworkLantern.Throughput' {
        Test-PathUnderBase -BasePath $TestDrive -CandidatePath $TestDrive | Should -Be $true
      }
    }

    It 'rejects paths outside the base' {
      InModuleScope 'NetworkLantern.Throughput' {
        $base = Join-Path $TestDrive 'sub'
        $outside = Join-Path $TestDrive 'other'
        Test-PathUnderBase -BasePath $base -CandidatePath $outside | Should -Be $false
      }
    }
  }

  Context 'ConvertTo-Iperf3HashtableFromObject' {
    It 'converts PSCustomObject to hashtable' {
      InModuleScope 'NetworkLantern.Throughput' {
        $obj = [pscustomobject]@{ A = 1; B = 'two' }
        $h = ConvertTo-Iperf3HashtableFromObject -InputObject $obj
        $h -is [hashtable] | Should -Be $true
        $h['A'] | Should -Be 1
        $h['B'] | Should -Be 'two'
      }
    }

    It 'passes through hashtable as-is' {
      InModuleScope 'NetworkLantern.Throughput' {
        $h = @{ X = 42 }
        $result = ConvertTo-Iperf3HashtableFromObject -InputObject $h
        $result['X'] | Should -Be 42
      }
    }

    It 'returns empty hashtable for null' {
      InModuleScope 'NetworkLantern.Throughput' {
        $result = ConvertTo-Iperf3HashtableFromObject -InputObject $null
        $result -is [hashtable] | Should -Be $true
        $result.Count | Should -Be 0
      }
    }
  }

  Context 'RetryCount config validation' {
    It 'accepts valid retry count' {
      InModuleScope 'NetworkLantern.Throughput' {
        $result = ConvertTo-Iperf3KnownValue -Key 'RetryCount' -Value 3
        $result | Should -Be 3
      }
    }

    It 'rejects retry count out of range' {
      InModuleScope 'NetworkLantern.Throughput' {
        { ConvertTo-Iperf3KnownValue -Key 'RetryCount' -Value 10 } | Should -Throw '*range*'
      }
    }
  }

  Context 'ConvertTo-Iperf3IntArray' {
    It 'converts valid values to int array' {
      InModuleScope 'NetworkLantern.Throughput' {
        $result = ConvertTo-Iperf3IntArray -Value @(1, 2, 3)
        $result.Count | Should -Be 3
        $result[0] | Should -Be 1
      }
    }

    It 'throws on non-integer value' {
      InModuleScope 'NetworkLantern.Throughput' {
        { ConvertTo-Iperf3IntArray -Value @('abc') } | Should -Throw
      }
    }
  }

  Context 'ConvertTo-Iperf3StringArray' {
    It 'strips whitespace and filters blanks' {
      InModuleScope 'NetworkLantern.Throughput' {
        $result = ConvertTo-Iperf3StringArray -Value @(' hello ', '', '  ', 'world')
        $result.Count | Should -Be 2
        $result[0] | Should -Be 'hello'
        $result[1] | Should -Be 'world'
      }
    }
  }
}
