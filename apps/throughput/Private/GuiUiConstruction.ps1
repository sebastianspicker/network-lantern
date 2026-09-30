[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Construction helpers create and style transient Windows Forms controls only.')]
param()

$colorWhite = [Drawing.Color]::White
$colorCanvas = [Drawing.ColorTranslator]::FromHtml('#F4F7F6')
$colorInk = [Drawing.ColorTranslator]::FromHtml('#17212B')
$colorMuted = [Drawing.ColorTranslator]::FromHtml('#667085')
$colorBorder = [Drawing.ColorTranslator]::FromHtml('#D0D8D5')
$colorPrimary = [Drawing.ColorTranslator]::FromHtml('#147D6F')

function New-GuiLabel {
  param([string]$Text, [string]$Name, [switch]$Muted, [switch]$Bold)
  $control = [Windows.Forms.Label]::new()
  $control.Text = $Text
  if ($Name) { $control.Name = $Name }
  $control.AutoSize = $true
  $control.Margin = [Windows.Forms.Padding]::new(3, 7, 8, 5)
  $control.ForeColor = if ($Muted) { $colorMuted } else { $colorInk }
  if ($Bold) { $control.Font = [Drawing.Font]::new($control.Font, [Drawing.FontStyle]::Bold) }
  return $control
}

function Set-GuiInputStyle {
  param([object]$Control, [string]$AccessibleName, [string]$Tip)
  $Control.Dock = 'Fill'
  $Control.Margin = [Windows.Forms.Padding]::new(3, 4, 10, 5)
  if ($AccessibleName) { $Control.AccessibleName = $AccessibleName }
  if ($Tip) { $toolTip.SetToolTip($Control, $Tip) }
}

function Set-GuiButtonStyle {
  param([Windows.Forms.Button]$Button, [ValidateSet('Primary', 'Secondary', 'Danger')][string]$Kind = 'Secondary')
  $Button.AutoSize = $true
  $Button.AutoSizeMode = 'GrowAndShrink'
  $Button.Padding = [Windows.Forms.Padding]::new(12, 5, 12, 5)
  $Button.Margin = [Windows.Forms.Padding]::new(0, 0, 8, 0)
  $Button.FlatStyle = 'Flat'
  $Button.FlatAppearance.BorderSize = 1
  if ($Kind -eq 'Primary') {
    $Button.BackColor = $colorPrimary
    $Button.ForeColor = $colorWhite
    $Button.FlatAppearance.BorderColor = [Drawing.ColorTranslator]::FromHtml('#0E5F55')
  }
  elseif ($Kind -eq 'Danger') {
    $Button.BackColor = $colorWhite
    $Button.ForeColor = [Drawing.ColorTranslator]::FromHtml('#B42318')
    $Button.FlatAppearance.BorderColor = [Drawing.ColorTranslator]::FromHtml('#FDA29B')
  }
  else {
    $Button.BackColor = $colorWhite
    $Button.ForeColor = [Drawing.ColorTranslator]::FromHtml('#0E5F55')
    $Button.FlatAppearance.BorderColor = $colorBorder
  }
}

function New-GuiGroup {
  param([string]$Text)
  $control = [Windows.Forms.GroupBox]::new()
  $control.Text = $Text
  $control.Dock = 'Fill'
  $control.AutoSize = $true
  $control.AutoSizeMode = 'GrowAndShrink'
  $control.Padding = [Windows.Forms.Padding]::new(12, 8, 12, 10)
  $control.Margin = [Windows.Forms.Padding]::new(0, 0, 0, 10)
  $control.ForeColor = $colorInk
  $control.BackColor = $colorWhite
  return $control
}

function New-GuiFourColumnTable {
  $control = [Windows.Forms.TableLayoutPanel]::new()
  $control.Dock = 'Fill'
  $control.AutoSize = $true
  $control.ColumnCount = 4
  $null = $control.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('AutoSize'))
  $null = $control.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('Percent', 50))
  $null = $control.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('AutoSize'))
  $null = $control.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('Percent', 50))
  return $control
}

$form = [Windows.Forms.Form]::new()
$form.Text = 'Network Lantern Throughput'
$form.ClientSize = [Drawing.Size]::new(1040, 820)
$form.MinimumSize = [Drawing.Size]::new(900, 700)
$form.StartPosition = 'CenterScreen'
$form.AutoScaleMode = 'Dpi'
$form.Font = [Drawing.Font]::new('Segoe UI', 9)
$form.BackColor = $colorCanvas
$form.ForeColor = $colorInk

$errorProvider = [Windows.Forms.ErrorProvider]::new()
$errorProvider.BlinkStyle = 'NeverBlink'
$errorProvider.ContainerControl = $form
$toolTip = [Windows.Forms.ToolTip]::new()
$toolTip.AutoPopDelay = 8000
$toolTip.InitialDelay = 400

$tabs = [Windows.Forms.TabControl]::new()
$tabs.Dock = 'Fill'
$tabs.Padding = [Drawing.Point]::new(16, 6)
$form.Controls.Add($tabs)
$tabRun = [Windows.Forms.TabPage]::new('Run')
$tabProfiles = [Windows.Forms.TabPage]::new('Profiles')
$tabReports = [Windows.Forms.TabPage]::new('Reports')
foreach ($tab in @($tabRun, $tabProfiles, $tabReports)) {
  $tab.BackColor = $colorCanvas
  $tab.Padding = [Windows.Forms.Padding]::new(12)
  $tabs.TabPages.Add($tab)
}
$tabRun.AutoScroll = $true

$runShell = [Windows.Forms.TableLayoutPanel]::new()
$runShell.Dock = 'Fill'
$runShell.ColumnCount = 1
$runShell.RowCount = 2
$null = $runShell.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('Percent', 100))
$null = $runShell.RowStyles.Add([Windows.Forms.RowStyle]::new('Percent', 100))
$null = $runShell.RowStyles.Add([Windows.Forms.RowStyle]::new('AutoSize'))
$tabRun.Controls.Add($runShell)

$runLayout = [Windows.Forms.TableLayoutPanel]::new()
$runLayout.Name = 'runLayout'
$runLayout.Dock = 'Fill'
$runLayout.AutoScroll = $true
$runLayout.ColumnCount = 1
$runLayout.RowCount = 6
$null = $runLayout.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('Percent', 100))
foreach ($index in 0..4) { $null = $runLayout.RowStyles.Add([Windows.Forms.RowStyle]::new('AutoSize')) }
$null = $runLayout.RowStyles.Add([Windows.Forms.RowStyle]::new('Percent', 100))
$runShell.Controls.Add($runLayout, 0, 0)

$grpConnection = New-GuiGroup 'Connection and run basics'; $grpConnection.Name = 'grpConnection'
$connectionTable = New-GuiFourColumnTable
$grpConnection.Controls.Add($connectionTable)
$runLayout.Controls.Add($grpConnection, 0, 0)
$lblTarget = New-GuiLabel 'Target'
$txtTarget = [Windows.Forms.TextBox]::new(); $txtTarget.Name = 'txtTarget'; $txtTarget.Text = '127.0.0.1'
Set-GuiInputStyle $txtTarget 'iperf3 server target' 'Hostname or IP address of the iperf3 server.'
$lblPort = New-GuiLabel 'Port'
$numPort = [Windows.Forms.NumericUpDown]::new(); $numPort.Name = 'numPort'; $numPort.Minimum = 1; $numPort.Maximum = 65535; $numPort.Value = 5201
Set-GuiInputStyle $numPort 'iperf3 server port'
$connectionTable.Controls.Add($lblTarget, 0, 0); $connectionTable.Controls.Add($txtTarget, 1, 0)
$connectionTable.Controls.Add($lblPort, 2, 0); $connectionTable.Controls.Add($numPort, 3, 0)

$lblOutDir = New-GuiLabel 'Output folder'
$outputPathLayout = [Windows.Forms.TableLayoutPanel]::new(); $outputPathLayout.Dock = 'Fill'; $outputPathLayout.AutoSize = $true; $outputPathLayout.ColumnCount = 2
$null = $outputPathLayout.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('Percent', 100)); $null = $outputPathLayout.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('AutoSize'))
$txtOutDir = [Windows.Forms.TextBox]::new(); $txtOutDir.Name = 'txtOutDir'; $txtOutDir.Text = Join-Path (Get-Location) 'logs'
Set-GuiInputStyle $txtOutDir 'Output folder'
$btnBrowseOut = [Windows.Forms.Button]::new(); $btnBrowseOut.Name = 'btnBrowseOut'; $btnBrowseOut.Text = 'Browse...'; $btnBrowseOut.AccessibleName = 'Browse for output folder'
Set-GuiButtonStyle $btnBrowseOut
$btnBrowseOut.Add_Click({
    $folder = [Windows.Forms.FolderBrowserDialog]::new()
    try { $folder.Description = 'Choose where throughput reports will be written.'; if ($folder.ShowDialog() -eq 'OK') { $txtOutDir.Text = $folder.SelectedPath } }
    finally { $folder.Dispose() }
  })
$outputPathLayout.Controls.Add($txtOutDir, 0, 0); $outputPathLayout.Controls.Add($btnBrowseOut, 1, 0)
$connectionTable.Controls.Add($lblOutDir, 0, 1); $connectionTable.Controls.Add($outputPathLayout, 1, 1); $connectionTable.SetColumnSpan($outputPathLayout, 3)

$lblDuration = New-GuiLabel 'Test length'
$timingFlow = [Windows.Forms.FlowLayoutPanel]::new(); $timingFlow.AutoSize = $true; $timingFlow.Dock = 'Fill'; $timingFlow.WrapContents = $false
$numDuration = [Windows.Forms.NumericUpDown]::new(); $numDuration.Name = 'numDuration'; $numDuration.Minimum = 1; $numDuration.Maximum = 3600; $numDuration.Value = 10; $numDuration.Width = 76
$numDuration.AccessibleName = 'Test duration in seconds'; $toolTip.SetToolTip($numDuration, 'Duration of each individual test.')
$numOmit = [Windows.Forms.NumericUpDown]::new(); $numOmit.Name = 'numOmit'; $numOmit.Minimum = 0; $numOmit.Maximum = 60; $numOmit.Value = 1; $numOmit.Width = 64
$numOmit.AccessibleName = 'Warm-up omit time in seconds'; $toolTip.SetToolTip($numOmit, 'Seconds omitted from the start of each test.')
$timingFlow.Controls.Add($numDuration); $timingFlow.Controls.Add((New-GuiLabel 'seconds +')); $timingFlow.Controls.Add($numOmit); $timingFlow.Controls.Add((New-GuiLabel 'seconds warm-up' -Muted))
$lblProtocol = New-GuiLabel 'Traffic'
$trafficFlow = [Windows.Forms.FlowLayoutPanel]::new(); $trafficFlow.AutoSize = $true; $trafficFlow.Dock = 'Fill'; $trafficFlow.WrapContents = $false
$comboProtocol = [Windows.Forms.ComboBox]::new(); $comboProtocol.Name = 'comboProtocol'; $comboProtocol.DropDownStyle = 'DropDownList'; $comboProtocol.Width = 100
@('Both', 'TCP', 'UDP') | ForEach-Object { [void]$comboProtocol.Items.Add($_) }; $comboProtocol.SelectedIndex = 0; $comboProtocol.AccessibleName = 'Traffic protocol'
$comboIpVersion = [Windows.Forms.ComboBox]::new(); $comboIpVersion.Name = 'comboIpVersion'; $comboIpVersion.DropDownStyle = 'DropDownList'; $comboIpVersion.Width = 100
@('Auto', 'IPv4', 'IPv6') | ForEach-Object { [void]$comboIpVersion.Items.Add($_) }; $comboIpVersion.SelectedIndex = 0; $comboIpVersion.AccessibleName = 'IP version'
$trafficFlow.Controls.Add($comboProtocol); $trafficFlow.Controls.Add((New-GuiLabel 'over' -Muted)); $trafficFlow.Controls.Add($comboIpVersion)
$connectionTable.Controls.Add($lblDuration, 0, 2); $connectionTable.Controls.Add($timingFlow, 1, 2)
$connectionTable.Controls.Add($lblProtocol, 2, 2); $connectionTable.Controls.Add($trafficFlow, 3, 2)

$optionsFlow = [Windows.Forms.FlowLayoutPanel]::new(); $optionsFlow.AutoSize = $true; $optionsFlow.Dock = 'Fill'; $optionsFlow.WrapContents = $true
$chkSingleTest = [Windows.Forms.CheckBox]::new(); $chkSingleTest.Name = 'chkSingleTest'; $chkSingleTest.Text = 'Single test only'
$chkProgress = [Windows.Forms.CheckBox]::new(); $chkProgress.Name = 'chkProgress'; $chkProgress.Text = 'Show progress details'; $chkProgress.Checked = $true
$chkSkipReach = [Windows.Forms.CheckBox]::new(); $chkSkipReach.Name = 'chkSkipReach'; $chkSkipReach.Text = 'Skip reachability check'
$chkDisableMtu = [Windows.Forms.CheckBox]::new(); $chkDisableMtu.Name = 'chkDisableMtu'; $chkDisableMtu.Text = 'Disable MTU probe'
$chkForce = [Windows.Forms.CheckBox]::new(); $chkForce.Name = 'chkForce'; $chkForce.Text = 'Overwrite existing outputs'
$chkStrict = [Windows.Forms.CheckBox]::new(); $chkStrict.Name = 'chkStrict'; $chkStrict.Text = 'Strict configuration'
foreach ($option in @($chkSingleTest, $chkProgress, $chkSkipReach, $chkDisableMtu, $chkForce, $chkStrict)) {
  $option.AutoSize = $true; $option.Margin = [Windows.Forms.Padding]::new(3, 5, 18, 3); $optionsFlow.Controls.Add($option)
}
$toolTip.SetToolTip($chkSingleTest, 'Run one quick test for connectivity validation.')
$toolTip.SetToolTip($chkSkipReach, 'Skip the ICMP/TCP pre-check.'); $toolTip.SetToolTip($chkDisableMtu, 'Skip the MTU payload probe.')
$toolTip.SetToolTip($chkForce, 'Replace existing output files with the same run name.'); $toolTip.SetToolTip($chkStrict, 'Reject unknown configuration and profile keys.')
$connectionTable.Controls.Add($optionsFlow, 0, 3); $connectionTable.SetColumnSpan($optionsFlow, 4)

$btnToggleAdvanced = [Windows.Forms.Button]::new(); $btnToggleAdvanced.Name = 'btnToggleAdvanced'; $btnToggleAdvanced.Text = 'Show advanced test matrix'
$btnToggleAdvanced.AccessibleName = 'Show or hide advanced test matrix settings'; Set-GuiButtonStyle $btnToggleAdvanced
$btnToggleAdvanced.Margin = [Windows.Forms.Padding]::new(0, 0, 0, 10); $runLayout.Controls.Add($btnToggleAdvanced, 0, 1)
$grpTestMatrix = New-GuiGroup 'Advanced test matrix'; $grpTestMatrix.Name = 'grpTestMatrix'; $grpTestMatrix.Visible = $false
$matrixTable = New-GuiFourColumnTable; $grpTestMatrix.Controls.Add($matrixTable); $runLayout.Controls.Add($grpTestMatrix, 0, 2)
$btnToggleAdvanced.Add_Click({ $grpTestMatrix.Visible = -not $grpTestMatrix.Visible; $btnToggleAdvanced.Text = if ($grpTestMatrix.Visible) { 'Hide advanced test matrix' } else { 'Show advanced test matrix' } })

$lblDscp = New-GuiLabel 'DSCP classes'
$txtDscpClasses = [Windows.Forms.TextBox]::new(); $txtDscpClasses.Name = 'txtDscpClasses'; $txtDscpClasses.Text = 'CS0,AF11,CS5,EF,AF41'
Set-GuiInputStyle $txtDscpClasses 'Comma-separated DSCP classes' 'Use CS0-CS7, EF, or AF11-AF43.'
$lblTcpWin = New-GuiLabel 'TCP windows'
$txtTcpWindows = [Windows.Forms.TextBox]::new(); $txtTcpWindows.Name = 'txtTcpWindows'; $txtTcpWindows.Text = 'default,128K,256K'
Set-GuiInputStyle $txtTcpWindows 'Comma-separated TCP window sizes' 'Use default or a whole number with an optional K, M, or G suffix.'
$matrixTable.Controls.Add($lblDscp, 0, 0); $matrixTable.Controls.Add($txtDscpClasses, 1, 0); $matrixTable.Controls.Add($lblTcpWin, 2, 0); $matrixTable.Controls.Add($txtTcpWindows, 3, 0)
$lblTcpStreams = New-GuiLabel 'TCP streams'
$txtTcpStreams = [Windows.Forms.TextBox]::new(); $txtTcpStreams.Name = 'txtTcpStreams'; $txtTcpStreams.Text = '1,4,8'
Set-GuiInputStyle $txtTcpStreams 'Comma-separated TCP stream counts' 'Use whole numbers from 1 to 128.'
$lblRetry = New-GuiLabel 'Retries per test'
$numRetryCount = [Windows.Forms.NumericUpDown]::new(); $numRetryCount.Name = 'numRetryCount'; $numRetryCount.Minimum = 0; $numRetryCount.Maximum = 5
Set-GuiInputStyle $numRetryCount 'Retries per test' 'Retry each transient iperf3 failure up to five times.'
$matrixTable.Controls.Add($lblTcpStreams, 0, 1); $matrixTable.Controls.Add($txtTcpStreams, 1, 1); $matrixTable.Controls.Add($lblRetry, 2, 1); $matrixTable.Controls.Add($numRetryCount, 3, 1)

$lblUdpRange = New-GuiLabel 'UDP saturation'
$udpFlow = [Windows.Forms.FlowLayoutPanel]::new(); $udpFlow.AutoSize = $true; $udpFlow.Dock = 'Fill'; $udpFlow.WrapContents = $true
$txtUdpStart = [Windows.Forms.TextBox]::new(); $txtUdpStart.Name = 'txtUdpStart'; $txtUdpStart.Text = '1M'; $txtUdpStart.Width = 80; $txtUdpStart.AccessibleName = 'UDP start bandwidth'
$txtUdpMax = [Windows.Forms.TextBox]::new(); $txtUdpMax.Name = 'txtUdpMax'; $txtUdpMax.Text = '1G'; $txtUdpMax.Width = 80; $txtUdpMax.AccessibleName = 'UDP maximum bandwidth'
$txtUdpStep = [Windows.Forms.TextBox]::new(); $txtUdpStep.Name = 'txtUdpStep'; $txtUdpStep.Text = '10M'; $txtUdpStep.Width = 80; $txtUdpStep.AccessibleName = 'UDP bandwidth step'
$numUdpLossThreshold = [Windows.Forms.NumericUpDown]::new(); $numUdpLossThreshold.Name = 'numUdpLossThreshold'; $numUdpLossThreshold.Minimum = 0; $numUdpLossThreshold.Maximum = 100
$numUdpLossThreshold.Value = 5; $numUdpLossThreshold.DecimalPlaces = 1; $numUdpLossThreshold.Width = 72; $numUdpLossThreshold.AccessibleName = 'UDP loss stop threshold percent'
$toolTip.SetToolTip($txtUdpStart, 'Starting bandwidth, such as 1M.'); $toolTip.SetToolTip($txtUdpMax, 'Maximum bandwidth, such as 1G.')
$toolTip.SetToolTip($txtUdpStep, 'Bandwidth increment, such as 10M.'); $toolTip.SetToolTip($numUdpLossThreshold, 'Stop a UDP ramp when loss exceeds this percentage.')
$udpFlow.Controls.Add((New-GuiLabel 'Start' -Muted)); $udpFlow.Controls.Add($txtUdpStart); $udpFlow.Controls.Add((New-GuiLabel 'Max' -Muted)); $udpFlow.Controls.Add($txtUdpMax)
$udpFlow.Controls.Add((New-GuiLabel 'Step' -Muted)); $udpFlow.Controls.Add($txtUdpStep); $udpFlow.Controls.Add((New-GuiLabel 'Stop above loss %' -Muted)); $udpFlow.Controls.Add($numUdpLossThreshold)
$matrixTable.Controls.Add($lblUdpRange, 0, 2); $matrixTable.Controls.Add($udpFlow, 1, 2); $matrixTable.SetColumnSpan($udpFlow, 3)

$grpGuardrails = New-GuiGroup 'Budget and result thresholds'; $grpGuardrails.Name = 'grpGuardrails'
$guardrailTable = New-GuiFourColumnTable; $grpGuardrails.Controls.Add($guardrailTable); $runLayout.Controls.Add($grpGuardrails, 0, 3)
$lblMaxTotalTests = New-GuiLabel 'Max tests'
$numMaxTotalTests = [Windows.Forms.NumericUpDown]::new(); $numMaxTotalTests.Name = 'numMaxTotalTests'; $numMaxTotalTests.Minimum = 0; $numMaxTotalTests.Maximum = 1000000
Set-GuiInputStyle $numMaxTotalTests 'Maximum total planned tests' 'Maximum planned tests for a live run. Zero means unlimited.'
$lblMaxTestsHelp = New-GuiLabel '0 means unlimited. Preview remains available when a plan exceeds this limit.' -Muted
$guardrailTable.Controls.Add($lblMaxTotalTests, 0, 0); $guardrailTable.Controls.Add($numMaxTotalTests, 1, 0); $guardrailTable.Controls.Add($lblMaxTestsHelp, 2, 0); $guardrailTable.SetColumnSpan($lblMaxTestsHelp, 2)
$lblThresholds = New-GuiLabel 'Pass/fail limits'
$thresholdFlow = [Windows.Forms.FlowLayoutPanel]::new(); $thresholdFlow.AutoSize = $true; $thresholdFlow.Dock = 'Fill'; $thresholdFlow.WrapContents = $true
$numThresholdMinTput = [Windows.Forms.NumericUpDown]::new(); $numThresholdMinTput.Name = 'numThresholdMinTput'; $numThresholdMinTput.Minimum = 0; $numThresholdMinTput.Maximum = 100000
$numThresholdMinTput.DecimalPlaces = 1; $numThresholdMinTput.Width = 90; $numThresholdMinTput.AccessibleName = 'Minimum throughput in megabits per second'
$numThresholdMaxLoss = [Windows.Forms.NumericUpDown]::new(); $numThresholdMaxLoss.Name = 'numThresholdMaxLoss'; $numThresholdMaxLoss.Minimum = -1; $numThresholdMaxLoss.Maximum = 100
$numThresholdMaxLoss.Value = -1; $numThresholdMaxLoss.DecimalPlaces = 1; $numThresholdMaxLoss.Width = 76; $numThresholdMaxLoss.AccessibleName = 'Maximum packet loss percentage'
$numThresholdMaxJitter = [Windows.Forms.NumericUpDown]::new(); $numThresholdMaxJitter.Name = 'numThresholdMaxJitter'; $numThresholdMaxJitter.Minimum = -1; $numThresholdMaxJitter.Maximum = 10000
$numThresholdMaxJitter.Value = -1; $numThresholdMaxJitter.DecimalPlaces = 1; $numThresholdMaxJitter.Width = 86; $numThresholdMaxJitter.AccessibleName = 'Maximum jitter in milliseconds'
$toolTip.SetToolTip($numThresholdMinTput, 'Minimum throughput. Zero disables this limit.'); $toolTip.SetToolTip($numThresholdMaxLoss, 'Maximum UDP loss. Minus one disables this limit.')
$toolTip.SetToolTip($numThresholdMaxJitter, 'Maximum jitter. Minus one disables this limit.')
$thresholdFlow.Controls.Add((New-GuiLabel 'Min Mbps' -Muted)); $thresholdFlow.Controls.Add($numThresholdMinTput); $thresholdFlow.Controls.Add((New-GuiLabel 'Max loss %' -Muted))
$thresholdFlow.Controls.Add($numThresholdMaxLoss); $thresholdFlow.Controls.Add((New-GuiLabel 'Max jitter ms' -Muted)); $thresholdFlow.Controls.Add($numThresholdMaxJitter)
$thresholdFlow.Controls.Add((New-GuiLabel '0 disables minimum; -1 disables maximums.' -Muted))
$guardrailTable.Controls.Add($lblThresholds, 0, 1); $guardrailTable.Controls.Add($thresholdFlow, 1, 1); $guardrailTable.SetColumnSpan($thresholdFlow, 3)

$grpPlanSummary = New-GuiGroup 'Plan preview'; $grpPlanSummary.BackColor = [Drawing.ColorTranslator]::FromHtml('#EFF7F5')
$planTable = [Windows.Forms.TableLayoutPanel]::new(); $planTable.Dock = 'Fill'; $planTable.AutoSize = $true; $planTable.ColumnCount = 1
$null = $planTable.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('Percent', 100))
$lblPlanHeadline = New-GuiLabel 'No plan preview yet' 'lblPlanHeadline' -Bold; $lblPlanHeadline.Font = [Drawing.Font]::new('Segoe UI Semibold', 10)
$lblPlanDetail = New-GuiLabel 'Preview the plan to confirm test count, nominal duration, and budget before running.' 'lblPlanDetail' -Muted; $lblPlanDetail.AutoSize = $false; $lblPlanDetail.Height = 38; $lblPlanDetail.Dock = 'Fill'
$planTable.Controls.Add($lblPlanHeadline, 0, 0); $planTable.Controls.Add($lblPlanDetail, 0, 1); $grpPlanSummary.Controls.Add($planTable); $runLayout.Controls.Add($grpPlanSummary, 0, 4)

$actionTable = [Windows.Forms.TableLayoutPanel]::new(); $actionTable.Dock = 'Fill'; $actionTable.AutoSize = $true; $actionTable.ColumnCount = 3; $actionTable.Margin = [Windows.Forms.Padding]::new(0, 0, 0, 10)
$null = $actionTable.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('AutoSize')); $null = $actionTable.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('Percent', 100)); $null = $actionTable.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('Absolute', 320))
$actionButtons = [Windows.Forms.FlowLayoutPanel]::new(); $actionButtons.AutoSize = $true; $actionButtons.WrapContents = $false
$btnWhatIf = [Windows.Forms.Button]::new(); $btnWhatIf.Name = 'btnWhatIf'; $btnWhatIf.Text = 'Preview plan'; $btnWhatIf.AccessibleName = 'Preview throughput test plan'; Set-GuiButtonStyle $btnWhatIf
$btnRun = [Windows.Forms.Button]::new(); $btnRun.Name = 'btnRun'; $btnRun.Text = 'Run tests'; $btnRun.AccessibleName = 'Run throughput tests'; Set-GuiButtonStyle $btnRun Primary
$btnCancel = [Windows.Forms.Button]::new(); $btnCancel.Name = 'btnCancel'; $btnCancel.Text = 'Cancel'; $btnCancel.AccessibleName = 'Cancel the current throughput operation'; $btnCancel.Enabled = $false; Set-GuiButtonStyle $btnCancel Danger
$actionButtons.Controls.Add($btnWhatIf); $actionButtons.Controls.Add($btnRun); $actionButtons.Controls.Add($btnCancel)
$progressBar = [Windows.Forms.ProgressBar]::new(); $progressBar.Name = 'progressBar'; $progressBar.Dock = 'Fill'; $progressBar.Minimum = 0; $progressBar.Maximum = 100
$progressBar.Margin = [Windows.Forms.Padding]::new(8, 8, 14, 8); $progressBar.AccessibleName = 'Throughput operation progress'
$statusLabel = New-GuiLabel 'Ready' 'statusLabel'; $statusLabel.AutoSize = $false; $statusLabel.Dock = 'Fill'; $statusLabel.TextAlign = 'MiddleLeft'; $statusLabel.AutoEllipsis = $true; $statusLabel.AccessibleName = 'Throughput operation status'
$statusLabel.Add_TextChanged({ $toolTip.SetToolTip($this, $this.Text) })
$actionTable.Controls.Add($actionButtons, 0, 0); $actionTable.Controls.Add($progressBar, 1, 0); $actionTable.Controls.Add($statusLabel, 2, 0); $runShell.Controls.Add($actionTable, 0, 1)

$grpLog = New-GuiGroup 'Run log'; $grpLog.AutoSize = $false; $grpLog.MinimumSize = [Drawing.Size]::new(0, 230)
$txtLog = [Windows.Forms.TextBox]::new(); $txtLog.Name = 'txtLog'; $txtLog.Multiline = $true; $txtLog.ScrollBars = 'Both'; $txtLog.WordWrap = $false; $txtLog.ReadOnly = $true; $txtLog.Dock = 'Fill'
$txtLog.BackColor = $colorWhite; $txtLog.ForeColor = $colorInk; $txtLog.Font = [Drawing.Font]::new('Consolas', 9); $txtLog.AccessibleName = 'Throughput run log'
$grpLog.Controls.Add($txtLog); $runLayout.Controls.Add($grpLog, 0, 5)

# Profiles retain their path provenance and public-module load/save behavior.
$profilesLayout = [Windows.Forms.TableLayoutPanel]::new(); $profilesLayout.Dock = 'Fill'; $profilesLayout.ColumnCount = 1; $profilesLayout.RowCount = 2
$null = $profilesLayout.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('Percent', 100)); $null = $profilesLayout.RowStyles.Add([Windows.Forms.RowStyle]::new('AutoSize')); $null = $profilesLayout.RowStyles.Add([Windows.Forms.RowStyle]::new('Percent', 100))
$tabProfiles.Controls.Add($profilesLayout)
$profileStoreGroup = New-GuiGroup 'Profile store'
$profileStoreTable = [Windows.Forms.TableLayoutPanel]::new(); $profileStoreTable.Dock = 'Fill'; $profileStoreTable.AutoSize = $true; $profileStoreTable.ColumnCount = 3
$null = $profileStoreTable.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('AutoSize')); $null = $profileStoreTable.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('Percent', 100)); $null = $profileStoreTable.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('AutoSize'))
$lblProfilesFile = New-GuiLabel 'Profiles file'
$txtProfilesFile = [Windows.Forms.TextBox]::new(); $txtProfilesFile.Name = 'txtProfilesFile'; $txtProfilesFile.Text = Join-Path (Join-Path (Get-Location) '.iperf3') 'profiles.json'; $txtProfilesFile.Tag = 'Default'
Set-GuiInputStyle $txtProfilesFile 'Profiles file path' 'The displayed default path is used unless you edit this field.'; $txtProfilesFile.Add_TextChanged({ $this.Tag = 'Explicit' })
$btnRefreshProfiles = [Windows.Forms.Button]::new(); $btnRefreshProfiles.Name = 'btnRefreshProfiles'; $btnRefreshProfiles.Text = 'Refresh list'; Set-GuiButtonStyle $btnRefreshProfiles
$profileStoreTable.Controls.Add($lblProfilesFile, 0, 0); $profileStoreTable.Controls.Add($txtProfilesFile, 1, 0); $profileStoreTable.Controls.Add($btnRefreshProfiles, 2, 0); $profileStoreGroup.Controls.Add($profileStoreTable); $profilesLayout.Controls.Add($profileStoreGroup, 0, 0)
$profileSplit = [Windows.Forms.SplitContainer]::new(); $profileSplit.Size = [Drawing.Size]::new(900, 500); $profileSplit.Dock = 'Fill'; $profileSplit.Orientation = 'Vertical'; $profileSplit.SplitterDistance = 430; $profileSplit.Panel1.Padding = [Windows.Forms.Padding]::new(0, 0, 8, 0); $profileSplit.Panel2.Padding = [Windows.Forms.Padding]::new(8, 0, 0, 0)
$profilesLayout.Controls.Add($profileSplit, 0, 1)
$profileListGroup = New-GuiGroup 'Saved profiles'; $profileListGroup.AutoSize = $false
$listProfiles = [Windows.Forms.ListBox]::new(); $listProfiles.Name = 'listProfiles'; $listProfiles.Dock = 'Fill'; $listProfiles.IntegralHeight = $false; $listProfiles.AccessibleName = 'Saved throughput profiles'
$profileListGroup.Controls.Add($listProfiles); $profileSplit.Panel1.Controls.Add($profileListGroup)
$profileActionsGroup = New-GuiGroup 'Selected profile'; $profileActionsGroup.AutoSize = $false
$profileActionsTable = [Windows.Forms.TableLayoutPanel]::new(); $profileActionsTable.Dock = 'Fill'; $profileActionsTable.AutoSize = $true; $profileActionsTable.ColumnCount = 1
$null = $profileActionsTable.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('Percent', 100))
$lblProfileName = New-GuiLabel 'Profile name'; $txtProfileName = [Windows.Forms.TextBox]::new(); $txtProfileName.Name = 'txtProfileName'; Set-GuiInputStyle $txtProfileName 'Profile name'
$profileHelp = New-GuiLabel 'Loading applies saved values to the Run tab and refreshes its plan preview.' -Muted
$profileButtons = [Windows.Forms.FlowLayoutPanel]::new(); $profileButtons.AutoSize = $true; $profileButtons.Dock = 'Fill'; $profileButtons.WrapContents = $true
$btnSaveProfile = [Windows.Forms.Button]::new(); $btnSaveProfile.Name = 'btnSaveProfile'; $btnSaveProfile.Text = 'Save current settings'; Set-GuiButtonStyle $btnSaveProfile Primary
$btnLoadProfile = [Windows.Forms.Button]::new(); $btnLoadProfile.Name = 'btnLoadProfile'; $btnLoadProfile.Text = 'Load selected'; Set-GuiButtonStyle $btnLoadProfile
$btnDeleteProfile = [Windows.Forms.Button]::new(); $btnDeleteProfile.Name = 'btnDeleteProfile'; $btnDeleteProfile.Text = 'Delete selected'; Set-GuiButtonStyle $btnDeleteProfile Danger
$profileButtons.Controls.Add($btnSaveProfile); $profileButtons.Controls.Add($btnLoadProfile); $profileButtons.Controls.Add($btnDeleteProfile)
$profileActionsTable.Controls.Add($lblProfileName, 0, 0); $profileActionsTable.Controls.Add($txtProfileName, 0, 1); $profileActionsTable.Controls.Add($profileHelp, 0, 2); $profileActionsTable.Controls.Add($profileButtons, 0, 3)
$profileActionsGroup.Controls.Add($profileActionsTable); $profileSplit.Panel2.Controls.Add($profileActionsGroup)

# Reports summarize the latest run and keep existing artifact-opening actions.
$reportsLayout = [Windows.Forms.TableLayoutPanel]::new(); $reportsLayout.Dock = 'Fill'; $reportsLayout.ColumnCount = 1; $reportsLayout.RowCount = 2
$null = $reportsLayout.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('Percent', 100)); $null = $reportsLayout.RowStyles.Add([Windows.Forms.RowStyle]::new('AutoSize')); $null = $reportsLayout.RowStyles.Add([Windows.Forms.RowStyle]::new('Percent', 100)); $tabReports.Controls.Add($reportsLayout)
$resultGroup = New-GuiGroup 'Last run'; $resultTable = [Windows.Forms.TableLayoutPanel]::new(); $resultTable.Dock = 'Fill'; $resultTable.AutoSize = $true
$lblRunResultHeadline = New-GuiLabel 'No completed run yet' 'lblRunResultHeadline' -Bold; $lblRunResultDetail = New-GuiLabel 'Run results and report locations will appear here.' 'lblRunResultDetail' -Muted
$resultTable.Controls.Add($lblRunResultHeadline, 0, 0); $resultTable.Controls.Add($lblRunResultDetail, 0, 1); $resultGroup.Controls.Add($resultTable); $reportsLayout.Controls.Add($resultGroup, 0, 0)
$artifactsGroup = New-GuiGroup 'Generated artifacts'; $artifactsGroup.AutoSize = $false
$artifactTable = [Windows.Forms.TableLayoutPanel]::new(); $artifactTable.Dock = 'Top'; $artifactTable.AutoSize = $true; $artifactTable.ColumnCount = 3
$null = $artifactTable.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('AutoSize')); $null = $artifactTable.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('Percent', 100)); $null = $artifactTable.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new('AutoSize'))
$lblLastSummary = New-GuiLabel 'Summary JSON'; $txtLastSummary = [Windows.Forms.TextBox]::new(); $txtLastSummary.Name = 'txtLastSummary'; $txtLastSummary.ReadOnly = $true; Set-GuiInputStyle $txtLastSummary 'Last summary JSON path'
$btnOpenSummary = [Windows.Forms.Button]::new(); $btnOpenSummary.Name = 'btnOpenSummary'; $btnOpenSummary.Text = 'Open summary'; Set-GuiButtonStyle $btnOpenSummary
$lblLastReport = New-GuiLabel 'Markdown report'; $txtLastReport = [Windows.Forms.TextBox]::new(); $txtLastReport.Name = 'txtLastReport'; $txtLastReport.ReadOnly = $true; Set-GuiInputStyle $txtLastReport 'Last Markdown report path'
$btnOpenReport = [Windows.Forms.Button]::new(); $btnOpenReport.Name = 'btnOpenReport'; $btnOpenReport.Text = 'Open report'; Set-GuiButtonStyle $btnOpenReport
$btnOpenOutDir = [Windows.Forms.Button]::new(); $btnOpenOutDir.Name = 'btnOpenOutDir'; $btnOpenOutDir.Text = 'Open output folder'; Set-GuiButtonStyle $btnOpenOutDir
$artifactTable.Controls.Add($lblLastSummary, 0, 0); $artifactTable.Controls.Add($txtLastSummary, 1, 0); $artifactTable.Controls.Add($btnOpenSummary, 2, 0)
$artifactTable.Controls.Add($lblLastReport, 0, 1); $artifactTable.Controls.Add($txtLastReport, 1, 1); $artifactTable.Controls.Add($btnOpenReport, 2, 1); $artifactTable.Controls.Add($btnOpenOutDir, 1, 2)
$artifactsGroup.Controls.Add($artifactTable); $reportsLayout.Controls.Add($artifactsGroup, 0, 1)

$btnOpenSummary.Add_Click({ if ($txtLastSummary.Text -and (Test-Path -LiteralPath $txtLastSummary.Text -PathType Leaf)) { Open-ThroughputFolderOrFile -Path $txtLastSummary.Text } })
$btnOpenReport.Add_Click({ if ($txtLastReport.Text -and (Test-Path -LiteralPath $txtLastReport.Text -PathType Leaf)) { Open-ThroughputFolderOrFile -Path $txtLastReport.Text } })
$btnOpenOutDir.Add_Click({ $dir = $txtOutDir.Text.Trim(); if ($dir -and (Test-Path -LiteralPath $dir -PathType Container)) { Open-ThroughputFolderOrFile -Path $dir } })

$markPreviewStale = { if (-not $script:RunJob) { Set-GuiPlanSummaryState -Form $form -State Stale -Preview $script:LastPlanPreview } }
foreach ($control in @($txtTarget, $txtOutDir, $txtDscpClasses, $txtTcpWindows, $txtTcpStreams, $txtUdpStart, $txtUdpMax, $txtUdpStep, $txtProfilesFile)) { $control.Add_TextChanged($markPreviewStale) }
foreach ($control in @($numPort, $numDuration, $numOmit, $numRetryCount, $numUdpLossThreshold, $numMaxTotalTests, $numThresholdMinTput, $numThresholdMaxLoss, $numThresholdMaxJitter)) { $control.Add_ValueChanged($markPreviewStale) }
foreach ($control in @($comboProtocol, $comboIpVersion)) { $control.Add_SelectedIndexChanged($markPreviewStale) }
foreach ($control in @($chkSingleTest, $chkProgress, $chkSkipReach, $chkDisableMtu, $chkForce, $chkStrict)) { $control.Add_CheckedChanged($markPreviewStale) }

$form.AcceptButton = $btnRun
Set-GuiPlanSummaryState -Form $form -State Empty
Set-GuiLastRunSummary -Form $form -Summary $null
