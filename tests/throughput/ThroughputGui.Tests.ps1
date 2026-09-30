BeforeAll {
  $script:RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
  . (Join-Path $script:RepoRoot 'apps/throughput/Private/GuiBudget.ps1')
  . (Join-Path $script:RepoRoot 'apps/throughput/Private/GuiPresentation.ps1')
  . (Join-Path $script:RepoRoot 'apps/throughput/Private/GuiProfiles.ps1')
}

Describe 'Throughput GUI pure input and presentation behavior' {
  It 'accepts the default matrix and reports actionable field-specific issues' {
    $valid = @{
      Target = 'example.local'; OutDir = 'logs'; DscpClasses = 'CS0,AF11,EF'
      TcpWindows = 'default,128K'; TcpStreams = '1,4,8'
      UdpStart = '1M'; UdpMax = '1G'; UdpStep = '10M'
    }
    @(Get-GuiRunInputIssues -Values $valid).Count | Should -Be 0

    $invalid = $valid.Clone()
    $invalid.Target = '-danger'
    $invalid.DscpClasses = 'CS0,bad'
    $invalid.TcpWindows = '12.5M'
    $invalid.TcpStreams = '1,zero,129'
    $invalid.UdpStep = 'fast'
    $issues = @(Get-GuiRunInputIssues -Values $invalid)
    @($issues.Field) | Should -Contain Target
    @($issues.Field) | Should -Contain DscpClasses
    @($issues.Field) | Should -Contain TcpWindows
    @($issues.Field) | Should -Contain TcpStreams
    @($issues.Field) | Should -Contain UdpStep
    ($issues | Where-Object Field -eq TcpStreams).Message | Should -Match 'whole number from 1 to 128'
  }

  It 'parses stream lists without culture-dependent coercion' {
    @(ConvertFrom-GuiTcpStreamsText '1, 4,128') | Should -Be @(1, 4, 128)
    { ConvertFrom-GuiTcpStreamsText '1,4.0' } | Should -Throw '*whole number*'
    { ConvertFrom-GuiTcpStreamsText '' } | Should -Throw '*at least one*'
  }

  It 'formats current, stale, unlimited, and over-budget plan summaries' {
    $within = [pscustomobject]@{ TotalApprox = 1145; EstimatedTestSeconds = 12595; MaxTotalTests = 1200; WithinTestBudget = $true }
    $unlimited = [pscustomobject]@{ TotalApprox = 1100; EstimatedTestSeconds = 12100; MaxTotalTests = 0; WithinTestBudget = $true }
    $over = [pscustomobject]@{ TotalApprox = 1145; EstimatedTestSeconds = 12595; MaxTotalTests = 1000; WithinTestBudget = $false }

    $withinModel = Get-GuiPlanSummaryModel -State Current -Preview $within
    $withinModel.Headline | Should -Be '1145 planned tests | 3h 29m 55s nominal'
    $withinModel.Detail | Should -Match 'Within test budget.*1145 of 1200'
    (Get-GuiPlanSummaryModel -State Current -Preview $unlimited).Detail | Should -Match 'No test limit'
    (Get-GuiPlanSummaryModel -State Current -Preview $over).Detail | Should -Match 'Over budget by 145 tests.*reduce the test matrix'
    (Get-GuiPlanSummaryModel -State Stale -Preview $within).Headline | Should -Match 'out of date'
    (Get-GuiPlanSummaryModel -State Stale).State | Should -Be Empty
  }

  It 'preserves zero-valued settings when applying profile parameters to controls' {
    $byName = @{}
    foreach ($name in @('txtTarget', 'txtOutDir', 'txtDscpClasses', 'txtTcpWindows', 'txtTcpStreams', 'txtUdpStart', 'txtUdpMax', 'txtUdpStep')) {
      $byName[$name] = [pscustomobject]@{ Text = '' }
    }
    foreach ($name in @('numPort', 'numDuration', 'numOmit', 'numRetryCount', 'numThresholdMinTput', 'numThresholdMaxLoss', 'numThresholdMaxJitter', 'numUdpLossThreshold', 'numMaxTotalTests')) {
      $byName[$name] = [pscustomobject]@{ Value = [decimal]99 }
    }
    foreach ($name in @('comboProtocol', 'comboIpVersion')) { $byName[$name] = [pscustomobject]@{ SelectedItem = $null } }
    foreach ($name in @('chkProgress', 'chkSkipReach', 'chkDisableMtu', 'chkSingleTest', 'chkForce', 'chkStrict')) { $byName[$name] = [pscustomobject]@{ Checked = $false } }
    $controls = [pscustomobject]@{ ByName = $byName }
    $controls | Add-Member ScriptMethod Find { param($name, $children) $null = $children; @($this.ByName[$name]) }
    $form = [pscustomobject]@{ Controls = $controls }
    $parameters = [pscustomobject]@{
      Target = 'example.local'; Port = 5201; OutDir = 'logs'; Duration = 10; Protocol = 'Both'; IpVersion = 'Auto'
      Progress = $true; SkipReachabilityCheck = $false; DisableMtuProbe = $false; SingleTest = $false; Force = $false; StrictConfiguration = $false
      Omit = 0; RetryCount = 0; DscpClasses = @('CS0'); TcpWindows = @('default'); TcpStreams = @(1)
      ThresholdMinThroughputMbps = 0; ThresholdMaxLossPct = 0; ThresholdMaxJitterMs = 0
      UdpStart = '1M'; UdpMax = '1G'; UdpStep = '10M'; UdpLossThreshold = 0; MaxTotalTests = 0
    }

    Set-GuiRunFormFromParameters -Form $form -Parameters $parameters
    $byName.numOmit.Value | Should -Be 0
    $byName.numUdpLossThreshold.Value | Should -Be 0
    $byName.numThresholdMaxLoss.Value | Should -Be 0
    $byName.numMaxTotalTests.Value | Should -Be 0
  }

  It 'passes the profile store through one splatted parameter during save' {
    $source = Get-Content (Join-Path $script:RepoRoot 'apps/throughput/Private/GuiProfiles.ps1') -Raw
    $source | Should -Match ([regex]::Escape("`$p['ProfilesFile'] = `$profilesPath"))
    $source | Should -Not -Match 'Measure-NetworkThroughput @p -ProfilesFile'
  }
}

Describe 'Throughput GUI composition and lifecycle contracts' {
  BeforeAll {
    $script:uiSource = Get-Content (Join-Path $script:RepoRoot 'apps/throughput/Private/GuiUiConstruction.ps1') -Raw
    $script:entrySource = Get-Content (Join-Path $script:RepoRoot 'apps/throughput/Measure-NetworkThroughput-GUI.ps1') -Raw
    $script:lifecycleSource = Get-Content (Join-Path $script:RepoRoot 'apps/throughput/Private/GuiRunLifecycle.ps1') -Raw
  }

  It 'uses responsive native layout with a docked action bar and progressive disclosure' {
    $script:uiSource | Should -Match 'TableLayoutPanel'
    $script:uiSource | Should -Match 'FlowLayoutPanel'
    $script:uiSource | Should -Match "MinimumSize = \[Drawing.Size\]::new\(900, 700\)"
    $script:uiSource | Should -Match '\$runShell\.Controls\.Add\(\$actionTable, 0, 1\)'
    $script:uiSource | Should -Not -Match '\$runLayout\.Controls\.Add\(\$actionTable'
    $script:uiSource | Should -Match ([regex]::Escape('$statusLabel.Add_TextChanged({ $toolTip.SetToolTip($this, $this.Text) })'))
    $script:uiSource | Should -Match ([regex]::Escape('$grpTestMatrix.Visible = $false'))
    $script:uiSource | Should -Match "Font = \[Drawing.Font\]::new\('Segoe UI'"
  }

  It 'retains every bound control name and clear primary action labels' {
    $names = @(
      'txtTarget', 'numPort', 'txtOutDir', 'numDuration', 'comboProtocol', 'comboIpVersion',
      'chkProgress', 'chkSkipReach', 'chkDisableMtu', 'chkSingleTest', 'chkForce', 'chkStrict',
      'numOmit', 'numRetryCount', 'txtDscpClasses', 'txtTcpWindows', 'numThresholdMinTput',
      'numThresholdMaxLoss', 'numThresholdMaxJitter', 'txtTcpStreams', 'txtUdpStart', 'txtUdpMax',
      'txtUdpStep', 'numUdpLossThreshold', 'numMaxTotalTests', 'btnRun', 'btnWhatIf', 'btnCancel',
      'txtProfilesFile', 'listProfiles', 'txtProfileName', 'btnSaveProfile', 'btnLoadProfile',
      'btnDeleteProfile', 'btnRefreshProfiles', 'txtLastSummary', 'txtLastReport'
    )
    foreach ($name in $names) { $script:uiSource | Should -Match "\.Name = '$name'" }
    $script:uiSource | Should -Match ([regex]::Escape("`$btnWhatIf.Text = 'Preview plan'"))
    $script:uiSource | Should -Match ([regex]::Escape("`$btnRun.Text = 'Run tests'"))
    $script:uiSource | Should -Match ([regex]::Escape("`$btnCancel.Text = 'Cancel'"))
  }

  It 'sizes the profile split before setting its distance and preserves path provenance' {
    $sizeIndex = $script:uiSource.IndexOf('$profileSplit.Size =')
    $distanceIndex = $script:uiSource.IndexOf('$profileSplit.SplitterDistance =')
    $sizeIndex | Should -BeGreaterThan -1
    $distanceIndex | Should -BeGreaterThan $sizeIndex
    $script:uiSource | Should -Match ([regex]::Escape("`$txtProfilesFile.Tag = 'Default'"))
    $script:uiSource | Should -Match ([regex]::Escape("`$txtProfilesFile.Add_TextChanged({ `$this.Tag = 'Explicit' })"))
  }

  It 'loads presentation before dependent helpers and preserves guarded cancellation wiring' {
    $presentationIndex = $script:entrySource.IndexOf("'GuiPresentation.ps1'")
    $formHelpersIndex = $script:entrySource.IndexOf("'GuiFormHelpers.ps1'")
    $presentationIndex | Should -BeGreaterThan -1
    $formHelpersIndex | Should -BeGreaterThan $presentationIndex
    ([regex]::Matches($script:entrySource, 'Stop-CurrentRunJob[^\r\n]+-Form \$form')).Count | Should -Be 3
    $script:entrySource | Should -Match 'Add_FormClosing'
  }

  It 'extracts validated parameters before entering busy state and recognizes authoritative previews' {
    $startIndex = $script:lifecycleSource.IndexOf('function Start-RunFromUi')
    $startSource = $script:lifecycleSource.Substring($startIndex)
    $parameterIndex = $startSource.IndexOf('$params = Get-ParamHashFromRunTab')
    $busyIndex = $startSource.IndexOf('Update-UiBusyState -Form $Form -Busy $true')
    $parameterIndex | Should -BeGreaterThan -1
    $busyIndex | Should -BeGreaterThan $parameterIndex
    $script:lifecycleSource | Should -Match ([regex]::Escape("Mode' -and `$item.Mode -eq 'WhatIf'"))
    $script:lifecycleSource | Should -Match 'Set-GuiPlanSummaryState -Form \$Form -State Current'
  }
}
