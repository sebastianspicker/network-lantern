BeforeAll {
  $script:Scanner = Join-Path $PSScriptRoot '../../scripts/Invoke-SecretScan.ps1'
  $script:Token = 'AK' + 'IA' + ('A' * 16)

  function Invoke-FixtureScan {
    param([string]$Path)
    $output = & pwsh -NoProfile -NonInteractive -File $script:Scanner -Path $Path 2>&1 | Out-String
    [pscustomobject]@{ Status = $LASTEXITCODE; Output = $output }
  }
}

Describe 'Secret scanner publication candidates' {
  It 'counts each pattern once per line, including repeated and case-insensitive tokens' {
    $fixture = New-Item -ItemType Directory -Path (Join-Path $TestDrive 'patterns')
    $tokens = @(
      ('-----' + 'BEGIN RSA PRIVATE KEY-----'),
      $script:Token,
      ('AS' + 'IA' + ('B' * 16)),
      ('gh' + 'p_' + ('c' * 36)),
      ('gh' + 'o_' + ('d' * 36)),
      ('gh' + 'u_' + ('e' * 36)),
      ('gh' + 's_' + ('f' * 36)),
      ('gh' + 'r_' + ('g' * 36)),
      ('github_' + 'pat_' + ('h' * 22)),
      ('xox' + 'b-' + ('i' * 10)),
      ('sk' + '-' + ('j' * 20)),
      ('AI' + 'za' + ('k' * 35)),
      ('ey' + 'J' + ('l' * 10) + '.ey' + 'J' + ('m' * 10) + '.' + ('n' * 10)),
      ('npm' + '_' + ('o' * 36)),
      ('pypi' + '-' + ('p' * 60)),
      ('SG' + '.' + ('q' * 22) + '.' + ('r' * 43)),
      ('sk_' + 'live_' + ('s' * 24)),
      ('rk_' + 'live_' + ('t' * 24))
    )
    $line = $tokens -join ' '
    [IO.File]::WriteAllText((Join-Path $fixture 'tokens.txt'), "$line $line`r`n$($line.ToLowerInvariant())")
    $result = Invoke-FixtureScan $fixture
    $result.Status | Should -Be 1
    $result.Output | Should -Match 'Potential secrets detected \(36\)'
    $result.Output | Should -Not -Match ([regex]::Escape($script:Token))
  }

  It 'detects BOM-encoded files and tolerates invalid UTF-8 as the original scanner did' {
    $fixture = New-Item -ItemType Directory -Path (Join-Path $TestDrive 'encodings')
    foreach ($encoding in @([Text.Encoding]::UTF8, [Text.Encoding]::Unicode, [Text.Encoding]::BigEndianUnicode, [Text.Encoding]::UTF32)) {
      [IO.File]::WriteAllText((Join-Path $fixture "$($encoding.CodePage).txt"), $script:Token, $encoding)
    }
    [IO.File]::WriteAllBytes((Join-Path $fixture 'invalid.txt'), ([byte[]]@(255, 10) + [Text.Encoding]::UTF8.GetBytes($script:Token)))
    $result = Invoke-FixtureScan $fixture
    $result.Status | Should -Be 1
    $result.Output | Should -Match 'Potential secrets detected \(5\)'
  }

  It 'keeps fallback exclusions and handles empty candidate sets' {
    $fixture = New-Item -ItemType Directory -Path (Join-Path $TestDrive 'fallback')
    (Invoke-FixtureScan $fixture).Status | Should -Be 0
    foreach ($excluded in @('.cache', 'artifacts')) {
      $directory = New-Item -ItemType Directory -Path (Join-Path $fixture $excluded)
      [IO.File]::WriteAllText((Join-Path $directory 'ignored.txt'), $script:Token)
    }
    [IO.File]::WriteAllText((Join-Path $fixture 'ignored.png'), $script:Token)
    [IO.File]::WriteAllText((Join-Path $fixture 'Invoke-SecretScan.ps1'), $script:Token)
    (Invoke-FixtureScan $fixture).Status | Should -Be 0
  }

  It 'scans tracked and untracked publication files but excludes ignored and deleted files' {
    $fixture = New-Item -ItemType Directory -Path (Join-Path $TestDrive 'git-fixture')
    & git -C $fixture init --quiet
    $LASTEXITCODE | Should -Be 0
    [IO.File]::WriteAllText((Join-Path $fixture '.gitignore'), "ignored.txt`n")
    [IO.File]::WriteAllText((Join-Path $fixture 'tracked.txt'), $script:Token)
    [IO.File]::WriteAllText((Join-Path $fixture 'deleted.txt'), $script:Token)
    & git -C $fixture add -- tracked.txt deleted.txt .gitignore
    $LASTEXITCODE | Should -Be 0
    Remove-Item -LiteralPath (Join-Path $fixture 'deleted.txt')
    [IO.File]::WriteAllText((Join-Path $fixture 'untracked.txt'), $script:Token)
    [IO.File]::WriteAllText((Join-Path $fixture 'ignored.txt'), $script:Token)
    $result = Invoke-FixtureScan $fixture
    $result.Status | Should -Be 1
    $result.Output | Should -Match 'Potential secrets detected \(2\)'
  }

  It 'fails when the requested root cannot be resolved' {
    (Invoke-FixtureScan (Join-Path $TestDrive 'missing')).Status | Should -Be 1
  }
}
