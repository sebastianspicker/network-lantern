function Enable-NetworkTuningLocalQosMarking {
  [CmdletBinding(SupportsShouldProcess = $true)]
  [OutputType([bool], [string])]
  param([switch]$DryRun)
  if ($DryRun) { Write-NetworkTuningInformation -Message '[DryRun] Enable local QoS marking (Do not use NLA).'; return $true }
  $qos = $script:NetworkTuningRegistryPathQos
  # 'Skipped' means the action was declined and nothing was applied.
  if (-not $PSCmdlet.ShouldProcess($qos, 'Enable local QoS marking (Do not use NLA)')) { return 'Skipped' }
  Set-NetworkTuningRegistryValue -Key $qos -Name 'Do not use NLA' -Type String -Value '1' -Confirm:$false
  return $true
}

function Set-NetworkTuningPowerPlan {
  [CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
  [OutputType([bool], [string])]
  param([Parameter(Mandatory)][ValidateSet('HighPerformance')][string]$PowerPlan, [switch]$DryRun)
  $guid = $script:NetworkTuningPowerPlanGuidHighPerformance
  if ($DryRun) { Write-NetworkTuningInformation -Message ("[DryRun] powercfg /S {0}" -f $guid); return $true }
  if (-not $PSCmdlet.ShouldProcess($PowerPlan, 'Set active power plan')) { return 'Skipped' }
  $null = & powercfg /S $guid 2>&1
  if ($LASTEXITCODE -ne 0) { Write-Warning -Message ("Could not switch your power plan. The selected plan may not be available on this PC (error code {0})." -f $LASTEXITCODE); return $false }
  return $true
}
