Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

BeforeAll {
  . "$PSScriptRoot/../../scripts/PathHelpers.ps1"
}

Describe 'Open-FolderOrFile native arguments' {
  It 'passes a path containing spaces and quotes as exactly one argument' -Skip:$IsWindows {
    $shimDirectory = Join-Path $TestDrive 'opener-bin'
    $capturePath = Join-Path $TestDrive 'captured-arguments.txt'
    $targetDirectory = Join-Path $TestDrive 'folder with spaces'
    $targetPath = Join-Path $targetDirectory 'report "quoted".txt'
    $openerName = if ($IsMacOS) { 'open' } else { 'xdg-open' }
    $openerPath = Join-Path $shimDirectory $openerName
    $originalPath = $env:PATH
    $originalCapture = $env:NETWORK_LANTERN_OPENER_CAPTURE

    New-Item -ItemType Directory -Path $shimDirectory, $targetDirectory -Force | Out-Null
    Set-Content -LiteralPath $targetPath -Value 'report' -Encoding UTF8
    Set-Content -LiteralPath $openerPath -Value @(
      '#!/usr/bin/env bash'
      'printf "%s\n" "$#" "$1" > "$NETWORK_LANTERN_OPENER_CAPTURE"'
    ) -Encoding UTF8
    & chmod +x $openerPath

    try {
      $env:NETWORK_LANTERN_OPENER_CAPTURE = $capturePath
      $env:PATH = "$shimDirectory$([System.IO.Path]::PathSeparator)$originalPath"
      Open-FolderOrFile -Path $targetPath

      foreach ($attempt in 1..40) {
        if (Test-Path -LiteralPath $capturePath) { break }
        Start-Sleep -Milliseconds 50
      }

      Test-Path -LiteralPath $capturePath | Should -BeTrue
      $captured = @(Get-Content -LiteralPath $capturePath)
      $captured[0] | Should -Be '1'
      $captured[1] | Should -Be ([System.IO.Path]::GetFullPath($targetPath))
    } finally {
      $env:PATH = $originalPath
      $env:NETWORK_LANTERN_OPENER_CAPTURE = $originalCapture
    }
  }
}
