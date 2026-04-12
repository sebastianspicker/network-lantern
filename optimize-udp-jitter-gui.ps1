<#
.SYNOPSIS
  Windows GUI for UDP Jitter Optimization (Apply, Backup, Restore, Reset).

.DESCRIPTION
  Launches a Windows Forms GUI that calls the WindowsUdpJitterOptimization module.
  Run PowerShell as Administrator for Apply/Backup/Restore/Reset to succeed.
  Windows only.

.NOTES
  Author: Sebastian J. Spicker
  License: MIT
#>

#Requires -Version 5.1

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not ($env:OS -eq 'Windows_NT' -or [Environment]::OSVersion.Platform -eq 'Win32NT')) {
  Write-Error 'This GUI runs on Windows only.'
  exit 1
}

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$script:ModuleLoaded = $false
$script:IsRunInProgress = $false
$script:IsAdministrator = $false
# Fallback when module not loaded; otherwise use Get-UjDefaultBackupFolder after load (see Form_Load)
$defaultBackupFolder = Join-Path -Path (if ([string]::IsNullOrWhiteSpace($env:ProgramData)) { 'C:\ProgramData' } else { $env:ProgramData }) -ChildPath 'UDPTune'

function Import-UjModuleOnce {
  [CmdletBinding()]
  [OutputType([bool])]
  param()

  if ($script:ModuleLoaded) { return $true }
  try {
    $manifestPath = Join-Path -Path $PSScriptRoot -ChildPath 'WindowsUdpJitterOptimization/WindowsUdpJitterOptimization.psd1'
    if (-not (Test-Path -LiteralPath $manifestPath)) {
      return $false
    }
    Import-Module -Name $manifestPath -Force -ErrorAction Stop
    $script:ModuleLoaded = $true
    return $true
  } catch {
    return $false
  }
}

function Get-UjActionFromCombo {
  [CmdletBinding()]
  [OutputType([string])]
  param()
  switch ($comboAction.SelectedIndex) {
    0 { 'Apply' }
    1 { 'Backup' }
    2 { 'Restore' }
    3 { 'ResetDefaults' }
    default { 'Apply' }
  }
}

function Show-UjValidationMessage {
  [CmdletBinding()]
  [OutputType([void])]
  param([Parameter(Mandatory)][string]$Message)
  [System.Windows.Forms.MessageBox]::Show($Message, 'Validation', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
}

function Update-UjRunButtonState {
  [CmdletBinding()]
  [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '')]
  [OutputType([void])]
  param()
  $btnRun.Enabled = $script:IsAdministrator -or $chkDryRun.Checked
}

$form = New-Object System.Windows.Forms.Form
$form.Text = 'UDP Jitter Optimization'
$form.Size = New-Object System.Drawing.Size(620, 648)
$form.MinimumSize = New-Object System.Drawing.Size(600, 548)
$form.StartPosition = 'CenterScreen'
$form.FormBorderStyle = 'Sizable'

$y = 12
$labelHeight = 20
$rowHeight = 28

# Action
$lblAction = New-Object System.Windows.Forms.Label
$lblAction.Location = New-Object System.Drawing.Point(12, $y)
$lblAction.Size = New-Object System.Drawing.Size(80, $labelHeight)
$lblAction.Text = 'Action:'
$form.Controls.Add($lblAction)
$comboAction = New-Object System.Windows.Forms.ComboBox
$comboAction.Location = New-Object System.Drawing.Point(100, $y - 2)
$comboAction.Size = New-Object System.Drawing.Size(220, 24)
$comboAction.DropDownStyle = 'DropDownList'
@('Apply Optimizations', 'Backup Current Settings', 'Restore from Backup', 'Reset to Windows Defaults') | ForEach-Object { [void]$comboAction.Items.Add($_) }
$comboAction.SelectedIndex = 0
$form.Controls.Add($comboAction)
$y += $rowHeight

# Preset (for Apply) - descriptive labels convey risk level at a glance
$lblPreset = New-Object System.Windows.Forms.Label
$lblPreset.Location = New-Object System.Drawing.Point(12, $y)
$lblPreset.Size = New-Object System.Drawing.Size(80, $labelHeight)
$lblPreset.Text = 'Preset:'
$form.Controls.Add($lblPreset)
$comboPreset = New-Object System.Windows.Forms.ComboBox
$comboPreset.Location = New-Object System.Drawing.Point(100, $y - 2)
$comboPreset.Size = New-Object System.Drawing.Size(380, 24)
$comboPreset.DropDownStyle = 'DropDownList'
@('1 - Safe (QoS + power-saving off)', '2 - Moderate (+ interrupt & flow tuning)', '3 - Aggressive (max optimization, higher CPU)') | ForEach-Object { [void]$comboPreset.Items.Add($_) }
$comboPreset.SelectedIndex = 0
$form.Controls.Add($comboPreset)
$y += $rowHeight

# Backup folder
$lblBackup = New-Object System.Windows.Forms.Label
$lblBackup.Location = New-Object System.Drawing.Point(12, $y)
$lblBackup.Size = New-Object System.Drawing.Size(80, $labelHeight)
$lblBackup.Text = 'Backup folder:'
$form.Controls.Add($lblBackup)
$txtBackup = New-Object System.Windows.Forms.TextBox
$txtBackup.Location = New-Object System.Drawing.Point(100, $y - 2)
$txtBackup.Size = New-Object System.Drawing.Size(380, 22)
$txtBackup.Text = $defaultBackupFolder
$txtBackup.Anchor = [System.Windows.Forms.AnchorStyles]::Top -bor [System.Windows.Forms.AnchorStyles]::Left -bor [System.Windows.Forms.AnchorStyles]::Right
$form.Controls.Add($txtBackup)
$btnBrowse = New-Object System.Windows.Forms.Button
$btnBrowse.Location = New-Object System.Drawing.Point(488, $y - 3)
$btnBrowse.Size = New-Object System.Drawing.Size(90, 24)
$btnBrowse.Text = 'Browse...'
$btnBrowse.Anchor = [System.Windows.Forms.AnchorStyles]::Top -bor [System.Windows.Forms.AnchorStyles]::Right
$form.Controls.Add($btnBrowse)
$y += $rowHeight

# Ports (TeamSpeak, CS2)
$lblPorts = New-Object System.Windows.Forms.Label
$lblPorts.Location = New-Object System.Drawing.Point(12, $y)
$lblPorts.Size = New-Object System.Drawing.Size(80, $labelHeight)
$lblPorts.Text = 'TeamSpeak port:'
$form.Controls.Add($lblPorts)
$txtTeamSpeakPort = New-Object System.Windows.Forms.TextBox
$txtTeamSpeakPort.Location = New-Object System.Drawing.Point(100, $y - 2)
$txtTeamSpeakPort.Size = New-Object System.Drawing.Size(60, 22)
$txtTeamSpeakPort.Text = '9987'
$form.Controls.Add($txtTeamSpeakPort)
$lblCS2 = New-Object System.Windows.Forms.Label
$lblCS2.Location = New-Object System.Drawing.Point(180, $y)
$lblCS2.Size = New-Object System.Drawing.Size(120, $labelHeight)
$lblCS2.Text = 'CS2 ports (start-end):'
$form.Controls.Add($lblCS2)
$txtCS2Start = New-Object System.Windows.Forms.TextBox
$txtCS2Start.Location = New-Object System.Drawing.Point(300, $y - 2)
$txtCS2Start.Size = New-Object System.Drawing.Size(50, 22)
$txtCS2Start.Text = '27015'
$form.Controls.Add($txtCS2Start)
$txtCS2End = New-Object System.Windows.Forms.TextBox
$txtCS2End.Location = New-Object System.Drawing.Point(358, $y - 2)
$txtCS2End.Size = New-Object System.Drawing.Size(50, 22)
$txtCS2End.Text = '27036'
$form.Controls.Add($txtCS2End)
$y += $rowHeight

# Power plan
$lblPower = New-Object System.Windows.Forms.Label
$lblPower.Location = New-Object System.Drawing.Point(12, $y)
$lblPower.Size = New-Object System.Drawing.Size(80, $labelHeight)
$lblPower.Text = 'Power plan:'
$form.Controls.Add($lblPower)
$comboPowerPlan = New-Object System.Windows.Forms.ComboBox
$comboPowerPlan.Location = New-Object System.Drawing.Point(100, $y - 2)
$comboPowerPlan.Size = New-Object System.Drawing.Size(120, 24)
$comboPowerPlan.DropDownStyle = 'DropDownList'
@('None (keep current)', 'High Performance', 'Ultimate Performance') | ForEach-Object { [void]$comboPowerPlan.Items.Add($_) }
$comboPowerPlan.SelectedIndex = 0
$form.Controls.Add($comboPowerPlan)
$lblAfd = New-Object System.Windows.Forms.Label
$lblAfd.Location = New-Object System.Drawing.Point(230, $y)
$lblAfd.Size = New-Object System.Drawing.Size(130, $labelHeight)
$lblAfd.Text = 'Packet size threshold:'
$form.Controls.Add($lblAfd)
$txtAfdThreshold = New-Object System.Windows.Forms.TextBox
$txtAfdThreshold.Location = New-Object System.Drawing.Point(362, $y - 2)
$txtAfdThreshold.Size = New-Object System.Drawing.Size(60, 22)
$txtAfdThreshold.Text = '1500'
$form.Controls.Add($txtAfdThreshold)
$y += $rowHeight

# Checkboxes
$chkDryRun = New-Object System.Windows.Forms.CheckBox
$chkDryRun.Location = New-Object System.Drawing.Point(12, $y)
$chkDryRun.Size = New-Object System.Drawing.Size(340, 22)
$chkDryRun.Text = 'Preview only (show what would change, no actual changes)'
$form.Controls.Add($chkDryRun)
$chkDisableGameDvr = New-Object System.Windows.Forms.CheckBox
$chkDisableGameDvr.Location = New-Object System.Drawing.Point(220, $y)
$chkDisableGameDvr.Size = New-Object System.Drawing.Size(200, 22)
$chkDisableGameDvr.Text = 'Disable Game DVR (frees CPU)'
$form.Controls.Add($chkDisableGameDvr)
$chkDisableUro = New-Object System.Windows.Forms.CheckBox
$chkDisableUro.Location = New-Object System.Drawing.Point(428, $y)
$chkDisableUro.Size = New-Object System.Drawing.Size(160, 22)
$chkDisableUro.Text = 'Disable URO (less batching)'
$form.Controls.Add($chkDisableUro)
$y += $rowHeight

# Experimental checkbox
$chkExperimental = New-Object System.Windows.Forms.CheckBox
$chkExperimental.Location = New-Object System.Drawing.Point(12, $y)
$chkExperimental.Size = New-Object System.Drawing.Size(560, 22)
$chkExperimental.Text = 'Include experimental settings (TCP tweaks, Wake-on-LAN off -- not proven to help)'
$form.Controls.Add($chkExperimental)
$y += $rowHeight

# Include app-based QoS policies
$chkIncludeAppPolicies = New-Object System.Windows.Forms.CheckBox
$chkIncludeAppPolicies.Location = New-Object System.Drawing.Point(12, $y)
$chkIncludeAppPolicies.Size = New-Object System.Drawing.Size(220, 22)
$chkIncludeAppPolicies.Text = 'Prioritize specific game/app .exe files'
$form.Controls.Add($chkIncludeAppPolicies)
$y += $rowHeight
$lblAppPaths = New-Object System.Windows.Forms.Label
$lblAppPaths.Location = New-Object System.Drawing.Point(12, $y)
$lblAppPaths.Size = New-Object System.Drawing.Size(350, $labelHeight)
$lblAppPaths.Text = 'Game/app paths (one .exe per line, e.g. C:\Games\cs2.exe):'
$form.Controls.Add($lblAppPaths)
$y += $labelHeight
$txtAppPaths = New-Object System.Windows.Forms.TextBox
$txtAppPaths.Location = New-Object System.Drawing.Point(12, $y)
$txtAppPaths.Multiline = $true
$txtAppPaths.Size = New-Object System.Drawing.Size(566, 44)
$txtAppPaths.ScrollBars = 'Vertical'
$txtAppPaths.Anchor = [System.Windows.Forms.AnchorStyles]::Top -bor [System.Windows.Forms.AnchorStyles]::Left -bor [System.Windows.Forms.AnchorStyles]::Right
$form.Controls.Add($txtAppPaths)
$y += 50

# Admin hint (set in Form_Load after module load so Test-UjIsAdministrator is available)
$lblAdmin = New-Object System.Windows.Forms.Label
$lblAdmin.Location = New-Object System.Drawing.Point(12, $y)
$lblAdmin.AutoSize = $true
$lblAdmin.Text = 'Loading...'
$form.Controls.Add($lblAdmin)
$y += $labelHeight + 6

# Run and Copy Log buttons
$btnRun = New-Object System.Windows.Forms.Button
$btnRun.Location = New-Object System.Drawing.Point(12, $y)
$btnRun.Size = New-Object System.Drawing.Size(100, 28)
$btnRun.Text = 'Run'
$form.Controls.Add($btnRun)
$btnCopyLog = New-Object System.Windows.Forms.Button
$btnCopyLog.Location = New-Object System.Drawing.Point(120, $y)
$btnCopyLog.Size = New-Object System.Drawing.Size(90, 28)
$btnCopyLog.Text = 'Copy Log'
$form.Controls.Add($btnCopyLog)
$y += 36

# Log area
$lblLog = New-Object System.Windows.Forms.Label
$lblLog.Location = New-Object System.Drawing.Point(12, $y)
$lblLog.Size = New-Object System.Drawing.Size(200, $labelHeight)
$lblLog.Text = 'Output:'
$form.Controls.Add($lblLog)
$y += $labelHeight
$txtLog = New-Object System.Windows.Forms.TextBox
$txtLog.Location = New-Object System.Drawing.Point(12, $y)
$txtLog.Multiline = $true
$txtLog.ScrollBars = 'Vertical'
$txtLog.ReadOnly = $true
$txtLog.Font = New-Object System.Drawing.Font('Consolas', 9)
$txtLog.Anchor = [System.Windows.Forms.AnchorStyles]::Top -bor [System.Windows.Forms.AnchorStyles]::Bottom -bor [System.Windows.Forms.AnchorStyles]::Left -bor [System.Windows.Forms.AnchorStyles]::Right
$txtLog.Size = New-Object System.Drawing.Size(566, 220)
$form.Controls.Add($txtLog)

# StatusStrip for progress feedback
$statusStrip = New-Object System.Windows.Forms.StatusStrip
$statusLabel = New-Object System.Windows.Forms.ToolStripStatusLabel
$statusLabel.Text = 'Ready'
[void]$statusStrip.Items.Add($statusLabel)
$form.Controls.Add($statusStrip)

$form.Add_Resize({
  $txtLog.Height = [Math]::Max(100, $form.ClientSize.Height - $y - 12 - $statusStrip.Height)
})

function Add-UjGuiLog {
  [CmdletBinding()]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)][string]$Phase,
    [Parameter(Mandatory)][string]$Message
  )
  $txtLog.AppendText(("[{0}] {1}`r`n" -f $Phase, $Message))
}

function Resolve-UjGuiRunParameter {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param()

  $action = Get-UjActionFromCombo
  $backupFolder = $txtBackup.Text.Trim()

  # Parse preset number from descriptive label (e.g. '1 (Conservative)' -> 1)
  $presetStr = $comboPreset.SelectedItem.ToString()
  $preset = if ($presetStr -match '^(\d+)') { [int]$Matches[1] } else { 1 }

  if ($action -in @('Apply', 'Backup', 'Restore')) {
    if ([string]::IsNullOrWhiteSpace($backupFolder)) {
      Show-UjValidationMessage -Message 'A backup folder is required so your current settings can be saved before changes. Please choose a folder.'
      return [pscustomobject]@{ IsValid = $false }
    }

    if (Get-Command -Name Test-UjUnsafeBackupFolder -ErrorAction SilentlyContinue) {
      if (Test-UjUnsafeBackupFolder -Path $backupFolder) {
        Show-UjValidationMessage -Message 'That backup folder is inside a Windows system directory, which could cause problems. Please choose a different folder (e.g. C:\UDPTune or your Documents folder).'
        return [pscustomobject]@{ IsValid = $false }
      }
    }
  }

  $teamSpeakPort = 9987
  $cs2Start = 27015
  $cs2End = 27036
  $afdThreshold = 1500

  if ($action -eq 'Apply') {
    try {
      if (-not [string]::IsNullOrWhiteSpace($txtTeamSpeakPort.Text)) { $teamSpeakPort = [int]$txtTeamSpeakPort.Text }
      if (-not [string]::IsNullOrWhiteSpace($txtCS2Start.Text)) { $cs2Start = [int]$txtCS2Start.Text }
      if (-not [string]::IsNullOrWhiteSpace($txtCS2End.Text)) { $cs2End = [int]$txtCS2End.Text }
    } catch {
      Show-UjValidationMessage -Message 'Ports must be valid numbers (1-65535).'
      return [pscustomobject]@{ IsValid = $false }
    }

    if ($teamSpeakPort -lt 1 -or $teamSpeakPort -gt 65535 -or
        $cs2Start -lt 1 -or $cs2Start -gt 65535 -or
        $cs2End -lt 1 -or $cs2End -gt 65535) {
      Show-UjValidationMessage -Message 'All ports must be between 1 and 65535.'
      return [pscustomobject]@{ IsValid = $false }
    }

    if ($cs2End -lt $cs2Start) {
      Show-UjValidationMessage -Message 'CS2 end port must be >= start port.'
      return [pscustomobject]@{ IsValid = $false }
    }

    # Informational warnings (non-blocking)
    if ($teamSpeakPort -ge $cs2Start -and $teamSpeakPort -le $cs2End) {
      Show-UjValidationMessage -Message ("Your TeamSpeak port ({0}) is inside your CS2 port range ({1}-{2}). Both will still get priority, but the rules may interfere with each other. This is usually fine, but you can change the ports if you have issues." -f $teamSpeakPort, $cs2Start, $cs2End)
    }

    $portCount = $cs2End - $cs2Start + 1
    if ($portCount -gt 100) {
      Show-UjValidationMessage -Message ("Your CS2 port range covers {0} ports. Only the first 100 will get individual priority rules. If you need broader coverage, check 'Prioritize specific game/app .exe files' and add your game executable instead." -f $portCount)
    }

    try {
      if (-not [string]::IsNullOrWhiteSpace($txtAfdThreshold.Text)) { $afdThreshold = [int]$txtAfdThreshold.Text }
    } catch {
      Show-UjValidationMessage -Message 'Packet size threshold must be a number between 0 and 65535 (default: 1500).'
      return [pscustomobject]@{ IsValid = $false }
    }

    if ($afdThreshold -lt 0 -or $afdThreshold -gt 65535) {
      Show-UjValidationMessage -Message 'Packet size threshold must be between 0 and 65535 (default: 1500).'
      return [pscustomobject]@{ IsValid = $false }
    }
  }

  $appPaths = @()
  if ($chkIncludeAppPolicies.Checked -and -not [string]::IsNullOrWhiteSpace($txtAppPaths.Text)) {
    $appPaths = $txtAppPaths.Text -split "`r?`n" | ForEach-Object { $_.Trim() } | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
  }

  $params = @{
    Action             = $action
    Preset             = $preset
    BackupFolder       = $backupFolder
    TeamSpeakPort      = $teamSpeakPort
    CS2PortStart       = $cs2Start
    CS2PortEnd         = $cs2End
    PowerPlan          = switch ($comboPowerPlan.SelectedIndex) { 0 { 'None' } 1 { 'HighPerformance' } 2 { 'Ultimate' } default { 'None' } }
    DisableGameDvr        = $chkDisableGameDvr.Checked
    DisableUro            = $chkDisableUro.Checked
    IncludeExperimental   = $chkExperimental.Checked
    DryRun                = $chkDryRun.Checked
    Confirm            = $false
    IncludeAppPolicies = $chkIncludeAppPolicies.Checked
    AppPaths           = $appPaths
    AfdThreshold       = $afdThreshold
  }

  return [pscustomobject]@{
    IsValid = $true
    Params  = $params
    Action  = $action
    Preset  = $preset
  }
}

function Update-UjControlState {
  [CmdletBinding()]
  [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '')]
  [OutputType([void])]
  param()
  $action = if ($comboAction.SelectedItem) { Get-UjActionFromCombo } else { 'Apply' }
  $isApply = $action -eq 'Apply'
  $needsBackupFolder = $action -in @('Apply', 'Backup', 'Restore')

  foreach ($control in @($lblPreset, $comboPreset, $lblPorts, $txtTeamSpeakPort, $lblCS2, $txtCS2Start, $txtCS2End, $lblPower, $comboPowerPlan, $lblAfd, $txtAfdThreshold, $chkDisableGameDvr, $chkDisableUro, $chkExperimental, $chkIncludeAppPolicies)) {
    $control.Enabled = $isApply
  }

  $appPathEnabled = $isApply -and $chkIncludeAppPolicies.Checked
  foreach ($control in @($lblAppPaths, $txtAppPaths)) {
    $control.Enabled = $appPathEnabled
  }

  foreach ($control in @($lblBackup, $txtBackup, $btnBrowse)) {
    $control.Enabled = $needsBackupFolder
  }
}

$form.Add_Load({
  if (Import-UjModuleOnce) {
    try {
      $mod = Get-Module -Name WindowsUdpJitterOptimization -ErrorAction SilentlyContinue
      if ($mod) { $form.Text = "UDP Jitter Optimization v$($mod.Version)" }
    } catch {
      # Keep default title when version is unavailable
      $null = $_
    }
    try {
      $script:defaultBackupFolder = Get-UjDefaultBackupFolder
      $txtBackup.Text = $script:defaultBackupFolder
    } catch {
      # Keep fallback path when Get-UjDefaultBackupFolder unavailable
      $null = $_
    }
    try {
      if (Test-UjIsAdministrator) {
        $script:IsAdministrator = $true
        $lblAdmin.Text = 'Running as Administrator.'
        $lblAdmin.ForeColor = [System.Drawing.Color]::DarkGreen
      } else {
        $script:IsAdministrator = $false
        $lblAdmin.Text = 'Not running as Admin -- close this, right-click PowerShell > "Run as Administrator", then relaunch.'
        $lblAdmin.ForeColor = [System.Drawing.Color]::DarkRed
      }
    } catch {
      $lblAdmin.Text = 'Could not determine elevation.'
      $lblAdmin.ForeColor = [System.Drawing.Color]::Gray
    }
  } else {
    $lblAdmin.Text = 'Module failed to load -- see error message for details.'
    $lblAdmin.ForeColor = [System.Drawing.Color]::DarkRed
    [System.Windows.Forms.MessageBox]::Show(
      ("Could not load the optimization module.`n`nMake sure the 'WindowsUdpJitterOptimization' folder is in the same directory as this script:`n{0}" -f $PSScriptRoot),
      'Module Not Found',
      [System.Windows.Forms.MessageBoxButtons]::OK,
      [System.Windows.Forms.MessageBoxIcon]::Warning
    )
  }

  Update-UjRunButtonState
  Update-UjControlState
})

$comboAction.Add_SelectedIndexChanged({ Update-UjControlState })
$chkIncludeAppPolicies.Add_CheckedChanged({ Update-UjControlState })
$chkDryRun.Add_CheckedChanged({ Update-UjRunButtonState })

$btnBrowse.Add_Click({
  $dialog = New-Object System.Windows.Forms.FolderBrowserDialog
  $dialog.Description = 'Select backup folder'
  $dialog.SelectedPath = $txtBackup.Text
  if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
    $txtBackup.Text = $dialog.SelectedPath
  }
})

$btnCopyLog.Add_Click({
  if (-not [string]::IsNullOrWhiteSpace($txtLog.Text)) {
    [System.Windows.Forms.Clipboard]::SetText($txtLog.Text)
    $statusLabel.Text = 'Log copied to clipboard.'
  }
})

$btnRun.Add_Click({
  if ($script:IsRunInProgress) {
    return
  }

  if (-not (Import-UjModuleOnce)) {
    [System.Windows.Forms.MessageBox]::Show("The optimization module could not be loaded. Make sure the 'WindowsUdpJitterOptimization' folder is next to this script.", 'Module Not Found', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Error)
    return
  }

  $action = Get-UjActionFromCombo
  if ($action -in @('Apply', 'Restore', 'ResetDefaults') -and -not $chkDryRun.Checked) {
    $confirmMsg =
      if ($action -eq 'Apply') { ("Apply preset {0}?`n`nThis will change Windows network and registry settings. Your current settings will be backed up first so you can undo this later." -f $comboPreset.SelectedItem) }
      elseif ($action -eq 'Restore') { "Restore your previous settings from backup?`n`nThis will revert all changes made by this tool." }
      else { "Reset everything to Windows defaults?`n`nThis removes all optimizations and restores stock Windows behavior." }
    $confirm = [System.Windows.Forms.MessageBox]::Show(
      $confirmMsg,
      'Confirm',
      [System.Windows.Forms.MessageBoxButtons]::YesNo,
      [System.Windows.Forms.MessageBoxIcon]::Question
    )
    if ($confirm -ne [System.Windows.Forms.DialogResult]::Yes) {
      return
    }
  }

  $resolved = Resolve-UjGuiRunParameter
  if (-not $resolved.IsValid) {
    return
  }

  $params = $resolved.Params
  $preset = $resolved.Preset
  $phase = if ($resolved.Action -eq 'ResetDefaults') { 'Reset' } else { $resolved.Action }

  $script:IsRunInProgress = $true
  $btnRun.Enabled = $false
  $statusLabel.Text = 'Running...'
  $form.Cursor = [System.Windows.Forms.Cursors]::WaitCursor
  $txtLog.Clear()
  Add-UjGuiLog -Phase 'Validate' -Message 'Inputs validated.'
  Add-UjGuiLog -Phase $phase -Message ("Running action (Preset {0})." -f $preset)

  $runSuccess = $true
  try {
    $InformationPreference = 'Continue'
    $output = Invoke-UdpJitterOptimization @params 6>&1 3>&1
    foreach ($line in @($output)) {
      Add-UjGuiLog -Phase 'Output' -Message ([string]$line)
    }
    Add-UjGuiLog -Phase 'Done' -Message 'Completed.'
    $statusLabel.Text = 'Completed.'
  } catch {
    $runSuccess = $false
    Add-UjGuiLog -Phase 'Error' -Message $_.Exception.Message
    if ($_.ScriptStackTrace) {
      Add-UjGuiLog -Phase 'Error' -Message $_.ScriptStackTrace
    }
    $statusLabel.Text = 'Finished with errors -- check the log above for details.'
  } finally {
    $form.Cursor = [System.Windows.Forms.Cursors]::Default
    $script:IsRunInProgress = $false
    Update-UjRunButtonState
  }

  # Reboot recommendation after successful non-DryRun system changes
  if ($runSuccess -and -not $chkDryRun.Checked -and $action -in @('Apply', 'Restore', 'ResetDefaults')) {
    [System.Windows.Forms.MessageBox]::Show(
      "Done! Restart your PC for all changes to take effect.`n`nSome settings (like NIC tuning) apply immediately, but registry-based tweaks need a reboot.",
      'Restart Recommended',
      [System.Windows.Forms.MessageBoxButtons]::OK,
      [System.Windows.Forms.MessageBoxIcon]::Information
    ) | Out-Null
  }
})

[void]$form.ShowDialog()
