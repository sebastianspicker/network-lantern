<#
.SYNOPSIS
  Graphical UI for iperf3 Test Suite (Windows Forms).
.DESCRIPTION
  Launches a Windows Forms GUI to configure and run Invoke-Iperf3TestSuite.
  Requires Windows and PowerShell 7+. Run with: pwsh -File .\iPerf3Test-GUI.ps1
#>
#Requires -Version 7.0
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseUsingScopeModifierInNewRunspaces', '', Justification = 'Start-Job receives arguments via -ArgumentList, not outer scope')]
param()

if (-not $IsWindows) {
  Write-Error 'GUI is only supported on Windows (System.Windows.Forms).'
  exit 1
}

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

[System.Windows.Forms.Application]::EnableVisualStyles()

$script:RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
. (Join-Path $script:RepoRoot 'scripts/PathHelpers.ps1')
$script:ModulePath = Join-Path $script:RepoRoot 'src/powershell/throughput/Iperf3TestSuite.psd1'
Import-Module $script:ModulePath -Force

$script:RunJob = $null
$script:RunJobIperf3Pid = $null
$script:LastRunSummary = $null
$script:RunStartTime = $null

function Get-ParamHashFromRunTab {
  param([System.Windows.Forms.Form]$Form)
  $target = $Form.Controls.Find('txtTarget', $true) | Select-Object -First 1
  $port = $Form.Controls.Find('numPort', $true) | Select-Object -First 1
  $outDir = $Form.Controls.Find('txtOutDir', $true) | Select-Object -First 1
  $duration = $Form.Controls.Find('numDuration', $true) | Select-Object -First 1
  $protocol = $Form.Controls.Find('comboProtocol', $true) | Select-Object -First 1
  $ipVersion = $Form.Controls.Find('comboIpVersion', $true) | Select-Object -First 1
  $chkProgress = $Form.Controls.Find('chkProgress', $true) | Select-Object -First 1
  $chkSkipReach = $Form.Controls.Find('chkSkipReach', $true) | Select-Object -First 1
  $chkDisableMtu = $Form.Controls.Find('chkDisableMtu', $true) | Select-Object -First 1
  $chkSingleTest = $Form.Controls.Find('chkSingleTest', $true) | Select-Object -First 1
  $chkForce = $Form.Controls.Find('chkForce', $true) | Select-Object -First 1
  $chkStrict = $Form.Controls.Find('chkStrict', $true) | Select-Object -First 1
  $profilesFile = $Form.Controls.Find('txtProfilesFile', $true) | Select-Object -First 1

  $omit = $Form.Controls.Find('numOmit', $true) | Select-Object -First 1
  $retryCount = $Form.Controls.Find('numRetryCount', $true) | Select-Object -First 1
  $dscpClasses = $Form.Controls.Find('txtDscpClasses', $true) | Select-Object -First 1
  $tcpWindows = $Form.Controls.Find('txtTcpWindows', $true) | Select-Object -First 1
  $threshMinTput = $Form.Controls.Find('numThresholdMinTput', $true) | Select-Object -First 1
  $threshMaxLoss = $Form.Controls.Find('numThresholdMaxLoss', $true) | Select-Object -First 1
  $threshMaxJitter = $Form.Controls.Find('numThresholdMaxJitter', $true) | Select-Object -First 1
  $tcpStreamsCtrl = $Form.Controls.Find('txtTcpStreams', $true) | Select-Object -First 1
  $udpStart = $Form.Controls.Find('txtUdpStart', $true) | Select-Object -First 1
  $udpMax = $Form.Controls.Find('txtUdpMax', $true) | Select-Object -First 1
  $udpStep = $Form.Controls.Find('txtUdpStep', $true) | Select-Object -First 1
  $udpLossThreshold = $Form.Controls.Find('numUdpLossThreshold', $true) | Select-Object -First 1

  $hash = @{
    Target                = $target.Text.Trim()
    Port                  = [int]$port.Value
    OutDir                = $outDir.Text.Trim()
    Duration              = [int]$duration.Value
    Omit                  = [int]$omit.Value
    Protocol              = $protocol.SelectedItem.ToString()
    IpVersion             = $ipVersion.SelectedItem.ToString()
    Progress              = $chkProgress.Checked
    SkipReachabilityCheck = $chkSkipReach.Checked
    DisableMtuProbe       = $chkDisableMtu.Checked
    SingleTest            = $chkSingleTest.Checked
    Force                 = $chkForce.Checked
    StrictConfiguration   = $chkStrict.Checked
    ProfilesFile          = $profilesFile.Text.Trim()
    RetryCount            = [int]$retryCount.Value
    DscpClasses           = @($dscpClasses.Text.Split(',') | ForEach-Object { $_.Trim() } | Where-Object { $_ })
    TcpWindows            = @($tcpWindows.Text.Split(',') | ForEach-Object { $_.Trim() } | Where-Object { $_ })
    TcpStreams             = @($tcpStreamsCtrl.Text.Split(',') | ForEach-Object { $_.Trim() } | Where-Object { $_ } | ForEach-Object { [int]$_ })
    UdpStart              = $udpStart.Text.Trim()
    UdpMax                = $udpMax.Text.Trim()
    UdpStep               = $udpStep.Text.Trim()
    UdpLossThreshold      = [double]$udpLossThreshold.Value
    Quiet                 = $false
  }
  if ([double]$threshMinTput.Value -gt 0) { $hash['ThresholdMinThroughputMbps'] = [double]$threshMinTput.Value }
  if ([double]$threshMaxLoss.Value -ge 0) { $hash['ThresholdMaxLossPct'] = [double]$threshMaxLoss.Value }
  if ([double]$threshMaxJitter.Value -ge 0) { $hash['ThresholdMaxJitterMs'] = [double]$threshMaxJitter.Value }
  return $hash
}

function Set-UiBusyState {
  param(
    [System.Windows.Forms.Form]$Form,
    [bool]$Busy
  )
  $btnRun = $Form.Controls.Find('btnRun', $true) | Select-Object -First 1
  $btnWhatIf = $Form.Controls.Find('btnWhatIf', $true) | Select-Object -First 1
  $btnCancel = $Form.Controls.Find('btnCancel', $true) | Select-Object -First 1
  $btnSaveProfile = $Form.Controls.Find('btnSaveProfile', $true) | Select-Object -First 1
  $btnLoadProfile = $Form.Controls.Find('btnLoadProfile', $true) | Select-Object -First 1
  $btnDeleteProfile = $Form.Controls.Find('btnDeleteProfile', $true) | Select-Object -First 1
  $btnRefreshProfiles = $Form.Controls.Find('btnRefreshProfiles', $true) | Select-Object -First 1

  foreach ($b in @($btnRun, $btnWhatIf, $btnSaveProfile, $btnLoadProfile, $btnDeleteProfile, $btnRefreshProfiles)) {
    if ($b) { $b.Enabled = -not $Busy }
  }
  if ($btnCancel) { $btnCancel.Enabled = $Busy }
}

function Test-RunFormValid {
  param(
    [System.Windows.Forms.Form]$Form,
    [System.Windows.Forms.ErrorProvider]$ErrorProvider
  )
  $target = $Form.Controls.Find('txtTarget', $true) | Select-Object -First 1
  $outDir = $Form.Controls.Find('txtOutDir', $true) | Select-Object -First 1
  $profilesFile = $Form.Controls.Find('txtProfilesFile', $true) | Select-Object -First 1
  $ok = $true
  $ErrorProvider.SetError($target, '')
  $ErrorProvider.SetError($outDir, '')
  if ($profilesFile) { $ErrorProvider.SetError($profilesFile, '') }
  if (-not $target.Text.Trim()) {
    $ErrorProvider.SetError($target, 'Target is required.')
    $ok = $false
  }
  elseif (-not (Test-ValidHostnameOrIP -Name $target.Text.Trim())) {
    $ErrorProvider.SetError($target, 'Invalid hostname or IP address.')
    $ok = $false
  }
  if (-not $outDir.Text.Trim()) {
    $ErrorProvider.SetError($outDir, 'Output directory is required.')
    $ok = $false
  }
  if ($profilesFile) {
    try {
      $null = Get-ProfilesFileFromForm -Form $Form
    }
    catch {
      $ErrorProvider.SetError($profilesFile, $_.Exception.Message)
      $ok = $false
    }
  }
  return $ok
}

function Start-SuiteJob {
  [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSupportsShouldProcess', '', Justification = 'WhatIf is passed through to the module, not implemented here')]
  param(
    [hashtable]$ParamHash,
    [switch]$WhatIf,
    [string]$ModulePath
  )
  $hash = $ParamHash.Clone()
  if ($WhatIf) { $hash['WhatIf'] = $true }
  $hash['PassThru'] = $true
  return (Start-Job -ScriptBlock {
      param($modPath, $params)
      Import-Module $modPath -Force
      Invoke-Iperf3TestSuite @params *>&1
    } -ArgumentList $ModulePath, $hash)
}

function Update-LogAndStateFromJob {
  param(
    [System.Windows.Forms.Form]$Form,
    [System.Management.Automation.Job]$Job,
    [System.Windows.Forms.TextBox]$LogBox,
    [System.Windows.Forms.ProgressBar]$ProgressBar,
    [System.Windows.Forms.Label]$StatusLabel,
    [System.Windows.Forms.Timer]$Timer
  )
  if (-not $Job) { return $false }

  $output = Receive-Job -Job $Job
  if ($output) {
    foreach ($item in @($output)) {
      if ($item -and $item.PSObject -and $item.PSObject.Properties.Name -contains 'ExitCode' -and $item.PSObject.Properties.Name -contains 'Status') {
        $script:LastRunSummary = $item
        $summaryPathBox = $Form.Controls.Find('txtLastSummary', $true) | Select-Object -First 1
        $reportPathBox = $Form.Controls.Find('txtLastReport', $true) | Select-Object -First 1
        if ($summaryPathBox -and $item.Supplemental.SummaryJsonPath) { $summaryPathBox.Text = [string]$item.Supplemental.SummaryJsonPath }
        if ($reportPathBox -and $item.Supplemental.ReportMdPath) { $reportPathBox.Text = [string]$item.Supplemental.ReportMdPath }
        continue
      }
      $line = [string]$item
      if ($line) {
        $LogBox.AppendText($line + "`r`n")
        if ($line -match 'Running test\s+(\d+)/(\d+)') {
          $current = [int]$matches[1]
          $total = [math]::Max([int]$matches[2], 1)
          $pct = [math]::Min(100, [int](100 * $current / $total))
          $ProgressBar.Value = $pct
          $elapsed = if ($script:RunStartTime) { [int]([datetime]::UtcNow - $script:RunStartTime).TotalSeconds } else { 0 }
          $StatusLabel.Text = "Running $current/$total ($pct%) ${elapsed}s"
        }
      }
    }
    $LogBox.ScrollToCaret()
  }

  if ($Job.State -eq 'Completed' -or $Job.State -eq 'Failed') {
    $Timer.Stop()
    Set-UiBusyState -Form $Form -Busy $false
    $ProgressBar.Value = 100
    $elapsed = if ($script:RunStartTime) { [int]([datetime]::UtcNow - $script:RunStartTime).TotalSeconds } else { 0 }
    if ($script:LastRunSummary) {
      $StatusLabel.Text = "Done: $($script:LastRunSummary.Status) (${elapsed}s)"
    }
    else {
      $StatusLabel.Text = "Done: $($Job.State) (${elapsed}s)"
    }
    Remove-Job -Job $Job -Force -ErrorAction SilentlyContinue
    return $true
  }

  if ($Job.State -eq 'Stopped') {
    $Timer.Stop()
    Set-UiBusyState -Form $Form -Busy $false
    $StatusLabel.Text = 'Cancelled'
    Remove-Job -Job $Job -Force -ErrorAction SilentlyContinue
    return $true
  }
  return $false
}

function Stop-CurrentRunJob {
  param(
    [System.Windows.Forms.Timer]$Timer,
    [System.Windows.Forms.Label]$StatusLabel
  )
  if (-not $script:RunJob) { return }
  try {
    # Kill only the iperf3 child process spawned by this job, not all iperf3 instances.
    # PowerShell background jobs do not directly expose child PIDs, so we attempt to
    # find iperf3 processes whose parent is the job's process. If unavailable, we log
    # a warning rather than killing all iperf3 processes on the system.
    if ($script:RunJobIperf3Pid) {
      try {
        $proc = Get-Process -Id $script:RunJobIperf3Pid -ErrorAction SilentlyContinue
        if ($proc -and -not $proc.HasExited) { $proc.Kill() }
      }
      catch { Write-Verbose "Failed to kill tracked iperf3 process $($script:RunJobIperf3Pid): $($_.Exception.Message)" }
    }
    else {
      Write-Warning "Cannot determine iperf3 child PID for the current job; orphaned iperf3 processes may remain. Check manually with 'Get-Process iperf3'."
    }
    Stop-Job -Job $script:RunJob -ErrorAction SilentlyContinue
    Remove-Job -Job $script:RunJob -Force -ErrorAction SilentlyContinue
  }
  finally {
    $script:RunJob = $null
    $script:RunJobIperf3Pid = $null
    if ($Timer) { $Timer.Tag = $null; $Timer.Stop() }
    if ($StatusLabel) { $StatusLabel.Text = 'Cancelled' }
  }
}

function Get-ProfilesFileFromForm {
  param([System.Windows.Forms.Form]$Form)
  $tb = $Form.Controls.Find('txtProfilesFile', $true) | Select-Object -First 1
  $path = $tb.Text.Trim()
  if (-not $path) { $path = (Join-Path (Join-Path (Get-Location) '.iperf3') 'profiles.json') }
  return (Resolve-ConfigPath -Path $path -BasePath (Get-Location).Path)
}

function Show-GuiError {
  param(
    [string]$Message,
    [string]$Title = 'Error'
  )
  [void][System.Windows.Forms.MessageBox]::Show($Message, $Title, [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Error)
}

function Update-ProfilesList {
  param([System.Windows.Forms.Form]$Form)
  try {
    $list = $Form.Controls.Find('listProfiles', $true) | Select-Object -First 1
    $profilesPath = Get-ProfilesFileFromForm -Form $Form
    $res = Invoke-Iperf3TestSuite -ListProfiles -ProfilesFile $profilesPath -PassThru -Quiet
    $list.Items.Clear()
    foreach ($n in @($res.Profiles)) { [void]$list.Items.Add($n) }
  }
  catch {
    Show-GuiError -Message $_.Exception.Message -Title 'Profiles'
  }
}

function Set-RunFormFromSelectedProfile {
  param([System.Windows.Forms.Form]$Form)
  try {
    $list = $Form.Controls.Find('listProfiles', $true) | Select-Object -First 1
    if (-not $list.SelectedItem) { return }
    $profileName = [string]$list.SelectedItem
    $profilesPath = Get-ProfilesFileFromForm -Form $Form
    $res = Invoke-Iperf3TestSuite -ProfileName $profileName -ProfilesFile $profilesPath -WhatIf -PassThru -Quiet
    $p = $res.EffectiveParameters
    if (-not $p) { return }

    ($Form.Controls.Find('txtTarget', $true) | Select-Object -First 1).Text = [string]$p.Target
    ($Form.Controls.Find('numPort', $true) | Select-Object -First 1).Value = [int]$p.Port
    ($Form.Controls.Find('txtOutDir', $true) | Select-Object -First 1).Text = [string]$p.OutDir
    ($Form.Controls.Find('numDuration', $true) | Select-Object -First 1).Value = [int]$p.Duration
    ($Form.Controls.Find('comboProtocol', $true) | Select-Object -First 1).SelectedItem = [string]$p.Protocol
    ($Form.Controls.Find('comboIpVersion', $true) | Select-Object -First 1).SelectedItem = [string]$p.IpVersion
    ($Form.Controls.Find('chkProgress', $true) | Select-Object -First 1).Checked = [bool]$p.Progress
    ($Form.Controls.Find('chkSkipReach', $true) | Select-Object -First 1).Checked = [bool]$p.SkipReachabilityCheck
    ($Form.Controls.Find('chkDisableMtu', $true) | Select-Object -First 1).Checked = [bool]$p.DisableMtuProbe
    ($Form.Controls.Find('chkSingleTest', $true) | Select-Object -First 1).Checked = [bool]$p.SingleTest
    ($Form.Controls.Find('chkForce', $true) | Select-Object -First 1).Checked = [bool]$p.Force
    ($Form.Controls.Find('chkStrict', $true) | Select-Object -First 1).Checked = [bool]$p.StrictConfiguration
    ($Form.Controls.Find('numOmit', $true) | Select-Object -First 1).Value = if ($p.Omit) { $p.Omit } else { 1 }
    ($Form.Controls.Find('numRetryCount', $true) | Select-Object -First 1).Value = if ($p.RetryCount) { $p.RetryCount } else { 0 }
    ($Form.Controls.Find('txtDscpClasses', $true) | Select-Object -First 1).Text = if ($p.DscpClasses) { ($p.DscpClasses -join ',') } else { 'CS0,AF11,CS5,EF,AF41' }
    ($Form.Controls.Find('txtTcpWindows', $true) | Select-Object -First 1).Text = if ($p.TcpWindows) { ($p.TcpWindows -join ',') } else { 'default,128K,256K' }
    ($Form.Controls.Find('numThresholdMinTput', $true) | Select-Object -First 1).Value = if ($p.ThresholdMinThroughputMbps) { $p.ThresholdMinThroughputMbps } else { 0 }
    ($Form.Controls.Find('numThresholdMaxLoss', $true) | Select-Object -First 1).Value = if ($null -ne $p.ThresholdMaxLossPct) { $p.ThresholdMaxLossPct } else { -1 }
    ($Form.Controls.Find('numThresholdMaxJitter', $true) | Select-Object -First 1).Value = if ($null -ne $p.ThresholdMaxJitterMs) { $p.ThresholdMaxJitterMs } else { -1 }
    ($Form.Controls.Find('txtTcpStreams', $true) | Select-Object -First 1).Text = if ($p.TcpStreams) { ($p.TcpStreams -join ',') } else { '1,4,8' }
    ($Form.Controls.Find('txtUdpStart', $true) | Select-Object -First 1).Text = if ($p.UdpStart) { $p.UdpStart } else { '1M' }
    ($Form.Controls.Find('txtUdpMax', $true) | Select-Object -First 1).Text = if ($p.UdpMax) { $p.UdpMax } else { '1G' }
    ($Form.Controls.Find('txtUdpStep', $true) | Select-Object -First 1).Text = if ($p.UdpStep) { $p.UdpStep } else { '10M' }
    ($Form.Controls.Find('numUdpLossThreshold', $true) | Select-Object -First 1).Value = if ($p.UdpLossThreshold) { $p.UdpLossThreshold } else { 5.0 }
  }
  catch {
    Show-GuiError -Message $_.Exception.Message -Title 'Profiles'
  }
}

function Save-ProfileFromForm {
  param([System.Windows.Forms.Form]$Form)
  try {
    $nameBox = $Form.Controls.Find('txtProfileName', $true) | Select-Object -First 1
    $profileName = $nameBox.Text.Trim()
    if (-not $profileName) {
      [System.Windows.Forms.MessageBox]::Show('Profile name is required.', 'Validation', 'OK', 'Warning')
      return
    }
    if ($profileName.Length -gt 128) {
      [System.Windows.Forms.MessageBox]::Show('Profile name must be 128 characters or fewer.', 'Validation', 'OK', 'Warning')
      return
    }
    if ($profileName -match '[/\\:\*\?"<>\|\x00]') {
      [System.Windows.Forms.MessageBox]::Show('Profile name contains invalid characters.', 'Validation', 'OK', 'Warning')
      return
    }
    $profilesPath = Get-ProfilesFileFromForm -Form $Form
    $p = Get-ParamHashFromRunTab -Form $Form
    $null = Invoke-Iperf3TestSuite @p -ProfilesFile $profilesPath -ProfileName $profileName -SaveProfile -WhatIf -PassThru -Quiet
    Update-ProfilesList -Form $Form
  }
  catch {
    Show-GuiError -Message $_.Exception.Message -Title 'Profiles'
  }
}

function Remove-SelectedProfile {
  param([System.Windows.Forms.Form]$Form)
  try {
    $list = $Form.Controls.Find('listProfiles', $true) | Select-Object -First 1
    if (-not $list.SelectedItem) { return }
    $profileName = [string]$list.SelectedItem
    $confirm = [System.Windows.Forms.MessageBox]::Show("Delete profile '$profileName'?", 'Confirm Delete', [System.Windows.Forms.MessageBoxButtons]::YesNo, [System.Windows.Forms.MessageBoxIcon]::Question)
    if ($confirm -ne [System.Windows.Forms.DialogResult]::Yes) { return }
    $profilesPath = Get-ProfilesFileFromForm -Form $Form
    $strict = ($Form.Controls.Find('chkStrict', $true) | Select-Object -First 1).Checked
    $removed = Remove-Iperf3Profile -ProfileName $profileName -ProfilesFile $profilesPath -StrictConfiguration:$strict
    if (-not $removed) {
      [void][System.Windows.Forms.MessageBox]::Show("Profile '$profileName' was not found.", 'Profiles', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Information)
    }
    Update-ProfilesList -Form $Form
  }
  catch {
    Show-GuiError -Message $_.Exception.Message -Title 'Profiles'
  }
}

$form = New-Object System.Windows.Forms.Form
$form.Text = 'iperf3 Test Suite'
$form.Size = New-Object System.Drawing.Size(880, 730)
$form.StartPosition = 'CenterScreen'
$form.AutoScaleMode = [System.Windows.Forms.AutoScaleMode]::Dpi

$errorProvider = New-Object System.Windows.Forms.ErrorProvider
$errorProvider.BlinkStyle = 'NeverBlink'

$toolTip = New-Object System.Windows.Forms.ToolTip
$toolTip.AutoPopDelay = 8000
$toolTip.InitialDelay = 400
$toolTip.ReshowDelay = 200

$tabs = New-Object System.Windows.Forms.TabControl
$tabs.Dock = 'Fill'
$form.Controls.Add($tabs)

$tabRun = New-Object System.Windows.Forms.TabPage
$tabRun.Text = 'Run'
$tabs.TabPages.Add($tabRun)

$tabProfiles = New-Object System.Windows.Forms.TabPage
$tabProfiles.Text = 'Profiles'
$tabs.TabPages.Add($tabProfiles)

$tabReports = New-Object System.Windows.Forms.TabPage
$tabReports.Text = 'Reports'
$tabs.TabPages.Add($tabReports)

# Run tab controls
$lblTarget = New-Object System.Windows.Forms.Label
$lblTarget.Text = 'Target:'
$lblTarget.Location = New-Object System.Drawing.Point(12, 14)
$tabRun.Controls.Add($lblTarget)

$txtTarget = New-Object System.Windows.Forms.TextBox
$txtTarget.Name = 'txtTarget'
$txtTarget.Location = New-Object System.Drawing.Point(100, 12)
$txtTarget.Size = New-Object System.Drawing.Size(220, 24)
$txtTarget.Text = '127.0.0.1'
$toolTip.SetToolTip($txtTarget, 'Hostname or IP of the iperf3 server')
$tabRun.Controls.Add($txtTarget)

$lblPort = New-Object System.Windows.Forms.Label
$lblPort.Text = 'Port:'
$lblPort.Location = New-Object System.Drawing.Point(340, 14)
$tabRun.Controls.Add($lblPort)

$numPort = New-Object System.Windows.Forms.NumericUpDown
$numPort.Name = 'numPort'
$numPort.Location = New-Object System.Drawing.Point(380, 12)
$numPort.Minimum = 1
$numPort.Maximum = 65535
$numPort.Value = 5201
$tabRun.Controls.Add($numPort)

$lblOutDir = New-Object System.Windows.Forms.Label
$lblOutDir.Text = 'Out dir:'
$lblOutDir.Location = New-Object System.Drawing.Point(12, 48)
$tabRun.Controls.Add($lblOutDir)

$txtOutDir = New-Object System.Windows.Forms.TextBox
$txtOutDir.Name = 'txtOutDir'
$txtOutDir.Location = New-Object System.Drawing.Point(100, 46)
$txtOutDir.Size = New-Object System.Drawing.Size(440, 24)
$txtOutDir.Text = (Join-Path (Get-Location) 'logs')
$tabRun.Controls.Add($txtOutDir)

$btnBrowseOut = New-Object System.Windows.Forms.Button
$btnBrowseOut.Text = '...'
$btnBrowseOut.Location = New-Object System.Drawing.Point(548, 44)
$btnBrowseOut.Size = New-Object System.Drawing.Size(34, 26)
$btnBrowseOut.Add_Click({
  $folder = New-Object System.Windows.Forms.FolderBrowserDialog
  if ($folder.ShowDialog() -eq 'OK') { $txtOutDir.Text = $folder.SelectedPath }
})
$tabRun.Controls.Add($btnBrowseOut)

$lblDuration = New-Object System.Windows.Forms.Label
$lblDuration.Text = 'Duration (s):'
$lblDuration.Location = New-Object System.Drawing.Point(12, 82)
$tabRun.Controls.Add($lblDuration)

$numDuration = New-Object System.Windows.Forms.NumericUpDown
$numDuration.Name = 'numDuration'
$numDuration.Location = New-Object System.Drawing.Point(100, 80)
$numDuration.Minimum = 1
$numDuration.Maximum = 3600
$numDuration.Value = 10
$toolTip.SetToolTip($numDuration, 'Duration of each individual test in seconds (1-3600)')
$tabRun.Controls.Add($numDuration)

$lblProtocol = New-Object System.Windows.Forms.Label
$lblProtocol.Text = 'Protocol:'
$lblProtocol.Location = New-Object System.Drawing.Point(200, 82)
$tabRun.Controls.Add($lblProtocol)

$comboProtocol = New-Object System.Windows.Forms.ComboBox
$comboProtocol.Name = 'comboProtocol'
$comboProtocol.Location = New-Object System.Drawing.Point(260, 80)
$comboProtocol.DropDownStyle = 'DropDownList'
@('Both', 'TCP', 'UDP') | ForEach-Object { [void]$comboProtocol.Items.Add($_) }
$comboProtocol.SelectedIndex = 0
$tabRun.Controls.Add($comboProtocol)

$lblIpVersion = New-Object System.Windows.Forms.Label
$lblIpVersion.Text = 'IP version:'
$lblIpVersion.Location = New-Object System.Drawing.Point(360, 82)
$tabRun.Controls.Add($lblIpVersion)

$comboIpVersion = New-Object System.Windows.Forms.ComboBox
$comboIpVersion.Name = 'comboIpVersion'
$comboIpVersion.Location = New-Object System.Drawing.Point(430, 80)
$comboIpVersion.DropDownStyle = 'DropDownList'
@('Auto', 'IPv4', 'IPv6') | ForEach-Object { [void]$comboIpVersion.Items.Add($_) }
$comboIpVersion.SelectedIndex = 0
$tabRun.Controls.Add($comboIpVersion)

$chkProgress = New-Object System.Windows.Forms.CheckBox
$chkProgress.Name = 'chkProgress'
$chkProgress.Text = 'Show progress'
$chkProgress.Location = New-Object System.Drawing.Point(12, 114)
$chkProgress.Checked = $true
$tabRun.Controls.Add($chkProgress)

$chkSkipReach = New-Object System.Windows.Forms.CheckBox
$chkSkipReach.Name = 'chkSkipReach'
$chkSkipReach.Text = 'Skip reachability check'
$chkSkipReach.Location = New-Object System.Drawing.Point(140, 114)
$chkSkipReach.Size = New-Object System.Drawing.Size(165, 20)
$toolTip.SetToolTip($chkSkipReach, 'Skip ICMP/TCP pre-check (required on Linux/macOS)')
$tabRun.Controls.Add($chkSkipReach)

$chkDisableMtu = New-Object System.Windows.Forms.CheckBox
$chkDisableMtu.Name = 'chkDisableMtu'
$chkDisableMtu.Text = 'Disable MTU probe'
$chkDisableMtu.Location = New-Object System.Drawing.Point(320, 114)
$toolTip.SetToolTip($chkDisableMtu, 'Skip MTU payload probe (required on Linux/macOS)')
$tabRun.Controls.Add($chkDisableMtu)

$chkSingleTest = New-Object System.Windows.Forms.CheckBox
$chkSingleTest.Name = 'chkSingleTest'
$chkSingleTest.Text = 'Single test only'
$chkSingleTest.Location = New-Object System.Drawing.Point(470, 114)
$toolTip.SetToolTip($chkSingleTest, 'Run one quick TCP test for connectivity validation')
$tabRun.Controls.Add($chkSingleTest)

$chkForce = New-Object System.Windows.Forms.CheckBox
$chkForce.Name = 'chkForce'
$chkForce.Text = 'Overwrite outputs'
$chkForce.Location = New-Object System.Drawing.Point(620, 114)
$toolTip.SetToolTip($chkForce, 'Overwrite existing output files instead of failing')
$tabRun.Controls.Add($chkForce)

$chkStrict = New-Object System.Windows.Forms.CheckBox
$chkStrict.Name = 'chkStrict'
$chkStrict.Text = 'Strict config'
$chkStrict.Location = New-Object System.Drawing.Point(620, 82)
$toolTip.SetToolTip($chkStrict, 'Error on unknown config/profile keys instead of warning')
$tabRun.Controls.Add($chkStrict)

# Advanced parameters row
$lblOmit = New-Object System.Windows.Forms.Label
$lblOmit.Text = 'Omit (s):'
$lblOmit.Location = New-Object System.Drawing.Point(12, 148)
$lblOmit.Size = New-Object System.Drawing.Size(50, 18)
$tabRun.Controls.Add($lblOmit)

$numOmit = New-Object System.Windows.Forms.NumericUpDown
$numOmit.Name = 'numOmit'
$numOmit.Location = New-Object System.Drawing.Point(64, 146)
$numOmit.Size = New-Object System.Drawing.Size(50, 24)
$numOmit.Minimum = 0; $numOmit.Maximum = 60; $numOmit.Value = 1
$toolTip.SetToolTip($numOmit, 'Seconds to omit from start of each test (warm-up exclusion)')
$tabRun.Controls.Add($numOmit)

$lblRetry = New-Object System.Windows.Forms.Label
$lblRetry.Text = 'Retries:'
$lblRetry.Location = New-Object System.Drawing.Point(122, 148)
$lblRetry.Size = New-Object System.Drawing.Size(44, 18)
$tabRun.Controls.Add($lblRetry)

$numRetryCount = New-Object System.Windows.Forms.NumericUpDown
$numRetryCount.Name = 'numRetryCount'
$numRetryCount.Location = New-Object System.Drawing.Point(168, 146)
$numRetryCount.Size = New-Object System.Drawing.Size(40, 24)
$numRetryCount.Minimum = 0; $numRetryCount.Maximum = 5; $numRetryCount.Value = 0
$toolTip.SetToolTip($numRetryCount, 'Number of retries per test on transient iperf3 failure (0-5)')
$tabRun.Controls.Add($numRetryCount)

$lblDscp = New-Object System.Windows.Forms.Label
$lblDscp.Text = 'DSCP:'
$lblDscp.Location = New-Object System.Drawing.Point(218, 148)
$lblDscp.Size = New-Object System.Drawing.Size(36, 18)
$tabRun.Controls.Add($lblDscp)

$txtDscpClasses = New-Object System.Windows.Forms.TextBox
$txtDscpClasses.Name = 'txtDscpClasses'
$txtDscpClasses.Location = New-Object System.Drawing.Point(256, 146)
$txtDscpClasses.Size = New-Object System.Drawing.Size(180, 24)
$txtDscpClasses.Text = 'CS0,AF11,CS5,EF,AF41'
$toolTip.SetToolTip($txtDscpClasses, 'Comma-separated DSCP class names (e.g., CS0,EF,AF41)')
$tabRun.Controls.Add($txtDscpClasses)

$lblTcpWin = New-Object System.Windows.Forms.Label
$lblTcpWin.Text = 'TCP win:'
$lblTcpWin.Location = New-Object System.Drawing.Point(444, 148)
$lblTcpWin.Size = New-Object System.Drawing.Size(50, 18)
$tabRun.Controls.Add($lblTcpWin)

$txtTcpWindows = New-Object System.Windows.Forms.TextBox
$txtTcpWindows.Name = 'txtTcpWindows'
$txtTcpWindows.Location = New-Object System.Drawing.Point(496, 146)
$txtTcpWindows.Size = New-Object System.Drawing.Size(140, 24)
$txtTcpWindows.Text = 'default,128K,256K'
$toolTip.SetToolTip($txtTcpWindows, 'Comma-separated TCP window sizes (e.g., default,128K,256K,512K)')
$tabRun.Controls.Add($txtTcpWindows)

# Threshold row (0 or -1 = disabled)
$lblThreshHeader = New-Object System.Windows.Forms.Label
$lblThreshHeader.Text = '(0 or -1 = disabled)'
$lblThreshHeader.Location = New-Object System.Drawing.Point(440, 178)
$lblThreshHeader.Size = New-Object System.Drawing.Size(140, 18)
$lblThreshHeader.ForeColor = [System.Drawing.Color]::Gray
$lblThreshHeader.Font = New-Object System.Drawing.Font($lblThreshHeader.Font, [System.Drawing.FontStyle]::Italic)
$toolTip.SetToolTip($lblThreshHeader, 'Set thresholds for CI pass/fail. 0 or -1 disables the threshold.')
$tabRun.Controls.Add($lblThreshHeader)

$lblMinTput = New-Object System.Windows.Forms.Label
$lblMinTput.Text = 'Min Mbps:'
$lblMinTput.Location = New-Object System.Drawing.Point(12, 178)
$lblMinTput.Size = New-Object System.Drawing.Size(60, 18)
$tabRun.Controls.Add($lblMinTput)

$numThresholdMinTput = New-Object System.Windows.Forms.NumericUpDown
$numThresholdMinTput.Name = 'numThresholdMinTput'
$numThresholdMinTput.Location = New-Object System.Drawing.Point(74, 176)
$numThresholdMinTput.Size = New-Object System.Drawing.Size(70, 24)
$numThresholdMinTput.Minimum = 0; $numThresholdMinTput.Maximum = 100000; $numThresholdMinTput.Value = 0
$numThresholdMinTput.DecimalPlaces = 1
$toolTip.SetToolTip($numThresholdMinTput, 'Minimum throughput (Mbps). 0 = disabled. Triggers exit code 14 if breached.')
$tabRun.Controls.Add($numThresholdMinTput)

$lblMaxLoss = New-Object System.Windows.Forms.Label
$lblMaxLoss.Text = 'Max loss%:'
$lblMaxLoss.Location = New-Object System.Drawing.Point(154, 178)
$lblMaxLoss.Size = New-Object System.Drawing.Size(64, 18)
$toolTip.SetToolTip($lblMaxLoss, '-1 = disabled')
$tabRun.Controls.Add($lblMaxLoss)

$numThresholdMaxLoss = New-Object System.Windows.Forms.NumericUpDown
$numThresholdMaxLoss.Name = 'numThresholdMaxLoss'
$numThresholdMaxLoss.Location = New-Object System.Drawing.Point(220, 176)
$numThresholdMaxLoss.Size = New-Object System.Drawing.Size(60, 24)
$numThresholdMaxLoss.Minimum = -1; $numThresholdMaxLoss.Maximum = 100; $numThresholdMaxLoss.Value = -1
$numThresholdMaxLoss.DecimalPlaces = 1
$toolTip.SetToolTip($numThresholdMaxLoss, 'Max allowed UDP loss (%). -1 = disabled. Triggers exit code 14 if breached.')
$tabRun.Controls.Add($numThresholdMaxLoss)

$lblMaxJitter = New-Object System.Windows.Forms.Label
$lblMaxJitter.Text = 'Max jitter ms:'
$lblMaxJitter.Location = New-Object System.Drawing.Point(290, 178)
$lblMaxJitter.Size = New-Object System.Drawing.Size(78, 18)
$toolTip.SetToolTip($lblMaxJitter, '-1 = disabled')
$tabRun.Controls.Add($lblMaxJitter)

$numThresholdMaxJitter = New-Object System.Windows.Forms.NumericUpDown
$numThresholdMaxJitter.Name = 'numThresholdMaxJitter'
$numThresholdMaxJitter.Location = New-Object System.Drawing.Point(370, 176)
$numThresholdMaxJitter.Size = New-Object System.Drawing.Size(60, 24)
$numThresholdMaxJitter.Minimum = -1; $numThresholdMaxJitter.Maximum = 10000; $numThresholdMaxJitter.Value = -1
$numThresholdMaxJitter.DecimalPlaces = 1
$toolTip.SetToolTip($numThresholdMaxJitter, 'Max allowed jitter (ms). -1 = disabled. Triggers exit code 14 if breached.')
$tabRun.Controls.Add($numThresholdMaxJitter)

# Advanced parameters row (TCP streams, UDP saturation range)
$lblTcpStreams = New-Object System.Windows.Forms.Label
$lblTcpStreams.Text = 'TCP streams:'
$lblTcpStreams.Location = New-Object System.Drawing.Point(12, 208)
$lblTcpStreams.Size = New-Object System.Drawing.Size(72, 18)
$tabRun.Controls.Add($lblTcpStreams)

$txtTcpStreams = New-Object System.Windows.Forms.TextBox
$txtTcpStreams.Name = 'txtTcpStreams'
$txtTcpStreams.Location = New-Object System.Drawing.Point(86, 206)
$txtTcpStreams.Size = New-Object System.Drawing.Size(50, 24)
$txtTcpStreams.Text = '1,4,8'
$toolTip.SetToolTip($txtTcpStreams, 'Comma-separated parallel TCP stream counts (e.g., 1,4,8,16)')
$tabRun.Controls.Add($txtTcpStreams)

$lblUdpStart = New-Object System.Windows.Forms.Label
$lblUdpStart.Text = 'UDP start:'
$lblUdpStart.Location = New-Object System.Drawing.Point(146, 208)
$lblUdpStart.Size = New-Object System.Drawing.Size(56, 18)
$tabRun.Controls.Add($lblUdpStart)

$txtUdpStart = New-Object System.Windows.Forms.TextBox
$txtUdpStart.Name = 'txtUdpStart'
$txtUdpStart.Location = New-Object System.Drawing.Point(204, 206)
$txtUdpStart.Size = New-Object System.Drawing.Size(56, 24)
$txtUdpStart.Text = '1M'
$toolTip.SetToolTip($txtUdpStart, 'Starting bandwidth for UDP saturation ramp (e.g., 1M, 10M)')
$tabRun.Controls.Add($txtUdpStart)

$lblUdpMax = New-Object System.Windows.Forms.Label
$lblUdpMax.Text = 'UDP max:'
$lblUdpMax.Location = New-Object System.Drawing.Point(268, 208)
$lblUdpMax.Size = New-Object System.Drawing.Size(54, 18)
$tabRun.Controls.Add($lblUdpMax)

$txtUdpMax = New-Object System.Windows.Forms.TextBox
$txtUdpMax.Name = 'txtUdpMax'
$txtUdpMax.Location = New-Object System.Drawing.Point(324, 206)
$txtUdpMax.Size = New-Object System.Drawing.Size(56, 24)
$txtUdpMax.Text = '1G'
$toolTip.SetToolTip($txtUdpMax, 'Maximum bandwidth for UDP saturation ramp (e.g., 100M, 1G)')
$tabRun.Controls.Add($txtUdpMax)

$lblUdpStep = New-Object System.Windows.Forms.Label
$lblUdpStep.Text = 'UDP step:'
$lblUdpStep.Location = New-Object System.Drawing.Point(388, 208)
$lblUdpStep.Size = New-Object System.Drawing.Size(54, 18)
$tabRun.Controls.Add($lblUdpStep)

$txtUdpStep = New-Object System.Windows.Forms.TextBox
$txtUdpStep.Name = 'txtUdpStep'
$txtUdpStep.Location = New-Object System.Drawing.Point(444, 206)
$txtUdpStep.Size = New-Object System.Drawing.Size(56, 24)
$txtUdpStep.Text = '10M'
$toolTip.SetToolTip($txtUdpStep, 'Bandwidth increment per UDP ramp step (e.g., 10M)')
$tabRun.Controls.Add($txtUdpStep)

$lblUdpLossThresh = New-Object System.Windows.Forms.Label
$lblUdpLossThresh.Text = 'UDP loss%:'
$lblUdpLossThresh.Location = New-Object System.Drawing.Point(508, 208)
$lblUdpLossThresh.Size = New-Object System.Drawing.Size(62, 18)
$tabRun.Controls.Add($lblUdpLossThresh)

$numUdpLossThreshold = New-Object System.Windows.Forms.NumericUpDown
$numUdpLossThreshold.Name = 'numUdpLossThreshold'
$numUdpLossThreshold.Location = New-Object System.Drawing.Point(572, 206)
$numUdpLossThreshold.Size = New-Object System.Drawing.Size(60, 24)
$numUdpLossThreshold.Minimum = 0; $numUdpLossThreshold.Maximum = 100; $numUdpLossThreshold.Value = 5
$numUdpLossThreshold.DecimalPlaces = 1
$toolTip.SetToolTip($numUdpLossThreshold, 'Stop UDP ramp when loss exceeds this % (0-100)')
$tabRun.Controls.Add($numUdpLossThreshold)

$btnRun = New-Object System.Windows.Forms.Button
$btnRun.Name = 'btnRun'
$btnRun.Text = 'Run'
$btnRun.Location = New-Object System.Drawing.Point(12, 240)
$btnRun.Size = New-Object System.Drawing.Size(90, 28)
$tabRun.Controls.Add($btnRun)

$btnWhatIf = New-Object System.Windows.Forms.Button
$btnWhatIf.Name = 'btnWhatIf'
$btnWhatIf.Text = 'WhatIf'
$btnWhatIf.Location = New-Object System.Drawing.Point(108, 240)
$btnWhatIf.Size = New-Object System.Drawing.Size(90, 28)
$tabRun.Controls.Add($btnWhatIf)

$btnCancel = New-Object System.Windows.Forms.Button
$btnCancel.Name = 'btnCancel'
$btnCancel.Text = 'Cancel'
$btnCancel.Location = New-Object System.Drawing.Point(204, 240)
$btnCancel.Size = New-Object System.Drawing.Size(70, 28)
$btnCancel.Enabled = $false
$tabRun.Controls.Add($btnCancel)

$progressBar = New-Object System.Windows.Forms.ProgressBar
$progressBar.Location = New-Object System.Drawing.Point(282, 243)
$progressBar.Size = New-Object System.Drawing.Size(348, 22)
$progressBar.Minimum = 0
$progressBar.Maximum = 100
$progressBar.Value = 0
$tabRun.Controls.Add($progressBar)

$statusLabel = New-Object System.Windows.Forms.Label
$statusLabel.Location = New-Object System.Drawing.Point(640, 244)
$statusLabel.Size = New-Object System.Drawing.Size(210, 22)
$statusLabel.Text = 'Idle'
$tabRun.Controls.Add($statusLabel)

$txtLog = New-Object System.Windows.Forms.TextBox
$txtLog.Multiline = $true
$txtLog.ScrollBars = 'Vertical'
$txtLog.ReadOnly = $true
$txtLog.Location = New-Object System.Drawing.Point(12, 276)
$txtLog.Size = New-Object System.Drawing.Size(840, 368)
$txtLog.Font = New-Object System.Drawing.Font('Consolas', 9)
$tabRun.Controls.Add($txtLog)

# Profiles tab controls
$lblProfilesFile = New-Object System.Windows.Forms.Label
$lblProfilesFile.Text = 'Profiles file:'
$lblProfilesFile.Location = New-Object System.Drawing.Point(12, 14)
$tabProfiles.Controls.Add($lblProfilesFile)

$txtProfilesFile = New-Object System.Windows.Forms.TextBox
$txtProfilesFile.Name = 'txtProfilesFile'
$txtProfilesFile.Location = New-Object System.Drawing.Point(100, 12)
$txtProfilesFile.Size = New-Object System.Drawing.Size(560, 24)
$txtProfilesFile.Text = (Join-Path (Join-Path (Get-Location) '.iperf3') 'profiles.json')
$tabProfiles.Controls.Add($txtProfilesFile)

$btnRefreshProfiles = New-Object System.Windows.Forms.Button
$btnRefreshProfiles.Name = 'btnRefreshProfiles'
$btnRefreshProfiles.Text = 'Refresh'
$btnRefreshProfiles.Location = New-Object System.Drawing.Point(670, 10)
$btnRefreshProfiles.Size = New-Object System.Drawing.Size(90, 28)
$tabProfiles.Controls.Add($btnRefreshProfiles)

$listProfiles = New-Object System.Windows.Forms.ListBox
$listProfiles.Name = 'listProfiles'
$listProfiles.Location = New-Object System.Drawing.Point(12, 48)
$listProfiles.Size = New-Object System.Drawing.Size(380, 520)
$tabProfiles.Controls.Add($listProfiles)

$lblProfileName = New-Object System.Windows.Forms.Label
$lblProfileName.Text = 'Profile name:'
$lblProfileName.Location = New-Object System.Drawing.Point(410, 50)
$tabProfiles.Controls.Add($lblProfileName)

$txtProfileName = New-Object System.Windows.Forms.TextBox
$txtProfileName.Name = 'txtProfileName'
$txtProfileName.Location = New-Object System.Drawing.Point(500, 48)
$txtProfileName.Size = New-Object System.Drawing.Size(260, 24)
$tabProfiles.Controls.Add($txtProfileName)

$btnSaveProfile = New-Object System.Windows.Forms.Button
$btnSaveProfile.Name = 'btnSaveProfile'
$btnSaveProfile.Text = 'Save current as profile'
$btnSaveProfile.Location = New-Object System.Drawing.Point(410, 84)
$btnSaveProfile.Size = New-Object System.Drawing.Size(170, 30)
$tabProfiles.Controls.Add($btnSaveProfile)

$btnLoadProfile = New-Object System.Windows.Forms.Button
$btnLoadProfile.Name = 'btnLoadProfile'
$btnLoadProfile.Text = 'Load selected profile'
$btnLoadProfile.Location = New-Object System.Drawing.Point(590, 84)
$btnLoadProfile.Size = New-Object System.Drawing.Size(170, 30)
$tabProfiles.Controls.Add($btnLoadProfile)

$btnDeleteProfile = New-Object System.Windows.Forms.Button
$btnDeleteProfile.Name = 'btnDeleteProfile'
$btnDeleteProfile.Text = 'Delete selected profile'
$btnDeleteProfile.Location = New-Object System.Drawing.Point(410, 120)
$btnDeleteProfile.Size = New-Object System.Drawing.Size(170, 30)
$tabProfiles.Controls.Add($btnDeleteProfile)

# Reports tab controls
$lblLastSummary = New-Object System.Windows.Forms.Label
$lblLastSummary.Text = 'Last summary JSON:'
$lblLastSummary.Location = New-Object System.Drawing.Point(12, 20)
$tabReports.Controls.Add($lblLastSummary)

$txtLastSummary = New-Object System.Windows.Forms.TextBox
$txtLastSummary.Name = 'txtLastSummary'
$txtLastSummary.Location = New-Object System.Drawing.Point(140, 18)
$txtLastSummary.Size = New-Object System.Drawing.Size(600, 24)
$txtLastSummary.ReadOnly = $true
$tabReports.Controls.Add($txtLastSummary)

$lblLastReport = New-Object System.Windows.Forms.Label
$lblLastReport.Text = 'Last report MD:'
$lblLastReport.Location = New-Object System.Drawing.Point(12, 54)
$tabReports.Controls.Add($lblLastReport)

$txtLastReport = New-Object System.Windows.Forms.TextBox
$txtLastReport.Name = 'txtLastReport'
$txtLastReport.Location = New-Object System.Drawing.Point(140, 52)
$txtLastReport.Size = New-Object System.Drawing.Size(600, 24)
$txtLastReport.ReadOnly = $true
$tabReports.Controls.Add($txtLastReport)

$btnOpenSummary = New-Object System.Windows.Forms.Button
$btnOpenSummary.Text = 'Open summary'
$btnOpenSummary.Location = New-Object System.Drawing.Point(140, 86)
$btnOpenSummary.Size = New-Object System.Drawing.Size(120, 30)
$btnOpenSummary.Add_Click({
  if ($txtLastSummary.Text -and (Test-Path -LiteralPath $txtLastSummary.Text -PathType Leaf)) {
    Open-FolderOrFile -Path $txtLastSummary.Text
  }
})
$tabReports.Controls.Add($btnOpenSummary)

$btnOpenReport = New-Object System.Windows.Forms.Button
$btnOpenReport.Text = 'Open report'
$btnOpenReport.Location = New-Object System.Drawing.Point(270, 86)
$btnOpenReport.Size = New-Object System.Drawing.Size(120, 30)
$btnOpenReport.Add_Click({
  if ($txtLastReport.Text -and (Test-Path -LiteralPath $txtLastReport.Text -PathType Leaf)) {
    Open-FolderOrFile -Path $txtLastReport.Text
  }
})
$tabReports.Controls.Add($btnOpenReport)

$btnOpenOutDir = New-Object System.Windows.Forms.Button
$btnOpenOutDir.Text = 'Open output folder'
$btnOpenOutDir.Location = New-Object System.Drawing.Point(400, 86)
$btnOpenOutDir.Size = New-Object System.Drawing.Size(130, 30)
$btnOpenOutDir.Add_Click({
  $dir = $txtOutDir.Text.Trim()
  if ($dir -and (Test-Path -LiteralPath $dir -PathType Container)) {
    Open-FolderOrFile -Path $dir
  }
})
$tabReports.Controls.Add($btnOpenOutDir)

$timer = New-Object System.Windows.Forms.Timer
$timer.Interval = 350
$timer.Add_Tick({
  if ($timer.Tag) {
    $done = Update-LogAndStateFromJob -Form $form -Job $timer.Tag -LogBox $txtLog -ProgressBar $progressBar -StatusLabel $statusLabel -Timer $timer
    if ($done) { $timer.Tag = $null }
  }
})

$btnRun.Add_Click({
  if (-not (Test-RunFormValid -Form $form -ErrorProvider $errorProvider)) { return }
  Set-UiBusyState -Form $form -Busy $true
  $progressBar.Value = 0
  $statusLabel.Text = 'Starting run...'
  $txtLog.Clear()
  $script:LastRunSummary = $null
  $script:RunStartTime = [datetime]::UtcNow
  $params = Get-ParamHashFromRunTab -Form $form
  $script:RunJob = Start-SuiteJob -ParamHash $params -ModulePath $script:ModulePath
  $timer.Tag = $script:RunJob
  $timer.Start()
})

$btnWhatIf.Add_Click({
  if (-not (Test-RunFormValid -Form $form -ErrorProvider $errorProvider)) { return }
  Set-UiBusyState -Form $form -Busy $true
  $progressBar.Value = 0
  $statusLabel.Text = 'WhatIf preview...'
  $txtLog.Clear()
  $script:LastRunSummary = $null
  $script:RunStartTime = [datetime]::UtcNow
  $params = Get-ParamHashFromRunTab -Form $form
  $script:RunJob = Start-SuiteJob -ParamHash $params -WhatIf -ModulePath $script:ModulePath
  $timer.Tag = $script:RunJob
  $timer.Start()
})

$btnCancel.Add_Click({
  Stop-CurrentRunJob -Timer $timer -StatusLabel $statusLabel
  $progressBar.Value = 0
  Set-UiBusyState -Form $form -Busy $false
})

$btnRefreshProfiles.Add_Click({ Update-ProfilesList -Form $form })
$btnLoadProfile.Add_Click({ Set-RunFormFromSelectedProfile -Form $form })
$btnSaveProfile.Add_Click({ Save-ProfileFromForm -Form $form })
$btnDeleteProfile.Add_Click({ Remove-SelectedProfile -Form $form })

$listProfiles.Add_SelectedIndexChanged({
  if ($listProfiles.SelectedItem) {
    $txtProfileName.Text = [string]$listProfiles.SelectedItem
  }
})

$form.Add_FormClosing({
  Stop-CurrentRunJob -Timer $timer -StatusLabel $statusLabel
  Set-UiBusyState -Form $form -Busy $false
})

Update-ProfilesList -Form $form
[void]$form.ShowDialog()

if ($script:RunJob) {
  Stop-Job -Job $script:RunJob -ErrorAction SilentlyContinue
  Remove-Job -Job $script:RunJob -Force -ErrorAction SilentlyContinue
}
if ($timer) { $timer.Stop() }
