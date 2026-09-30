[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Profile actions retain their existing confirmation dialogs and operate only on operator-selected profile files.')]
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '', Justification = 'Parameters names the complete profile parameter collection.')]
param()

function Get-ProfilesFileFromForm {
  param([System.Windows.Forms.Form]$Form)
  $tb = $Form.Controls.Find('txtProfilesFile', $true) | Select-Object -First 1
  $path = $tb.Text.Trim()
  # The initial expanded display is a convenience, not an explicit operator
  # destination. Once edited, even back to the same text, it is explicit.
  if (-not $path -or $tb.Tag -eq 'Default') { return $null }
  return $path
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
    $res = Measure-NetworkThroughput -ListProfiles -ProfilesFile $profilesPath -PassThru -Quiet
    $list.Items.Clear()
    foreach ($n in @($res.Profiles)) { [void]$list.Items.Add($n) }
  }
  catch {
    Show-GuiError -Message $_.Exception.Message -Title 'Profiles'
  }
}

function Set-GuiRunFormFromParameters {
  param(
    [Parameter(Mandatory)][object]$Form,
    [Parameter(Mandatory)][object]$Parameters
  )

  ($Form.Controls.Find('txtTarget', $true) | Select-Object -First 1).Text = [string]$Parameters.Target
  ($Form.Controls.Find('numPort', $true) | Select-Object -First 1).Value = [int]$Parameters.Port
  ($Form.Controls.Find('txtOutDir', $true) | Select-Object -First 1).Text = [string]$Parameters.OutDir
  ($Form.Controls.Find('numDuration', $true) | Select-Object -First 1).Value = [int]$Parameters.Duration
  ($Form.Controls.Find('comboProtocol', $true) | Select-Object -First 1).SelectedItem = [string]$Parameters.Protocol
  ($Form.Controls.Find('comboIpVersion', $true) | Select-Object -First 1).SelectedItem = [string]$Parameters.IpVersion
  ($Form.Controls.Find('chkProgress', $true) | Select-Object -First 1).Checked = [bool]$Parameters.Progress
  ($Form.Controls.Find('chkSkipReach', $true) | Select-Object -First 1).Checked = [bool]$Parameters.SkipReachabilityCheck
  ($Form.Controls.Find('chkDisableMtu', $true) | Select-Object -First 1).Checked = [bool]$Parameters.DisableMtuProbe
  ($Form.Controls.Find('chkSingleTest', $true) | Select-Object -First 1).Checked = [bool]$Parameters.SingleTest
  ($Form.Controls.Find('chkForce', $true) | Select-Object -First 1).Checked = [bool]$Parameters.Force
  ($Form.Controls.Find('chkStrict', $true) | Select-Object -First 1).Checked = [bool]$Parameters.StrictConfiguration
  ($Form.Controls.Find('numOmit', $true) | Select-Object -First 1).Value = [decimal](Get-GuiProfileValueOrDefault -Parameters $Parameters -Name 'Omit' -Default 1)
  ($Form.Controls.Find('numRetryCount', $true) | Select-Object -First 1).Value = [decimal](Get-GuiProfileValueOrDefault -Parameters $Parameters -Name 'RetryCount' -Default 0)
  ($Form.Controls.Find('txtDscpClasses', $true) | Select-Object -First 1).Text = @(Get-GuiProfileValueOrDefault -Parameters $Parameters -Name 'DscpClasses' -Default @('CS0', 'AF11', 'CS5', 'EF', 'AF41')) -join ','
  ($Form.Controls.Find('txtTcpWindows', $true) | Select-Object -First 1).Text = @(Get-GuiProfileValueOrDefault -Parameters $Parameters -Name 'TcpWindows' -Default @('default', '128K', '256K')) -join ','
  ($Form.Controls.Find('numThresholdMinTput', $true) | Select-Object -First 1).Value = [decimal](Get-GuiProfileValueOrDefault -Parameters $Parameters -Name 'ThresholdMinThroughputMbps' -Default 0)
  ($Form.Controls.Find('numThresholdMaxLoss', $true) | Select-Object -First 1).Value = [decimal](Get-GuiProfileValueOrDefault -Parameters $Parameters -Name 'ThresholdMaxLossPct' -Default -1)
  ($Form.Controls.Find('numThresholdMaxJitter', $true) | Select-Object -First 1).Value = [decimal](Get-GuiProfileValueOrDefault -Parameters $Parameters -Name 'ThresholdMaxJitterMs' -Default -1)
  ($Form.Controls.Find('txtTcpStreams', $true) | Select-Object -First 1).Text = @(Get-GuiProfileValueOrDefault -Parameters $Parameters -Name 'TcpStreams' -Default @(1, 4, 8)) -join ','
  ($Form.Controls.Find('txtUdpStart', $true) | Select-Object -First 1).Text = [string](Get-GuiProfileValueOrDefault -Parameters $Parameters -Name 'UdpStart' -Default '1M')
  ($Form.Controls.Find('txtUdpMax', $true) | Select-Object -First 1).Text = [string](Get-GuiProfileValueOrDefault -Parameters $Parameters -Name 'UdpMax' -Default '1G')
  ($Form.Controls.Find('txtUdpStep', $true) | Select-Object -First 1).Text = [string](Get-GuiProfileValueOrDefault -Parameters $Parameters -Name 'UdpStep' -Default '10M')
  ($Form.Controls.Find('numUdpLossThreshold', $true) | Select-Object -First 1).Value = [decimal](Get-GuiProfileValueOrDefault -Parameters $Parameters -Name 'UdpLossThreshold' -Default 5.0)
  Set-GuiMaxTotalTests -Form $Form -Value ([int](Get-GuiProfileValueOrDefault -Parameters $Parameters -Name 'MaxTotalTests' -Default 0))
}

function Set-RunFormFromSelectedProfile {
  param([System.Windows.Forms.Form]$Form)
  try {
    $list = $Form.Controls.Find('listProfiles', $true) | Select-Object -First 1
    if (-not $list.SelectedItem) { return }
    $profileName = [string]$list.SelectedItem
    $profilesPath = Get-ProfilesFileFromForm -Form $Form
    $res = Measure-NetworkThroughput -ProfileName $profileName -ProfilesFile $profilesPath -WhatIf -PassThru -Quiet
    $p = $res.EffectiveParameters
    if (-not $p) { return }

    Set-GuiRunFormFromParameters -Form $Form -Parameters $p
    $script:LastPlanPreview = $res
    Set-GuiPlanSummaryState -Form $Form -State Current -Preview $res
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
    $p['ProfilesFile'] = $profilesPath
    $null = Measure-NetworkThroughput @p -ProfileName $profileName -SaveProfile -WhatIf -PassThru -Quiet
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
