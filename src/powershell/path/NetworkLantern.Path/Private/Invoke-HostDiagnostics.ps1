function Get-NativeStageOutcome {
  <#
  .SYNOPSIS
    Return @{ Ok; Reason } for a native stage: timeout first, then (optionally) parsed output, else exit code.
  #>
  param(
    [Parameter(Mandatory)][object]$Result,
    [string]$TraceDestination
  )

  if ($Result.TimedOut) {
    return [pscustomobject]@{ Ok = $false; Reason = "timed out after $($Result.TimeoutSec) s" }
  }
  if ($TraceDestination) {
    return (Test-TraceReachedDestination -Raw $Result.Raw -Destination $TraceDestination)
  }
  if ($Result.ExitCode -eq 0) {
    return [pscustomobject]@{ Ok = $true; Reason = '' }
  }
  return [pscustomobject]@{ Ok = $false; Reason = "exit code $($Result.ExitCode)" }
}

function Invoke-HostDiagnostics {
  <#
  .SYNOPSIS
    Run the full diagnostic suite (ping, tracert, pathping, and TCP 443) for one plan item.
  .PARAMETER PlanItem
    A single plan entry from Get-DiagnosticPlan.
  .OUTPUTS
    [pscustomobject] Consolidated result with ping/tracert/pathping/TCP findings.
  #>
  param(
    [Parameter(Mandatory)][object]$PlanItem,
    [Parameter(Mandatory)][object]$Settings
  )

  $round = $PlanItem.RoundDef
  $proto = $PlanItem.Protocol
  $h = $PlanItem.Host
  $progressId = 2
  $activity = "round=$($round.Name) protocol=$proto host=$h"

  try {
    Write-Progress -Id $progressId -ParentId 1 -Activity $activity -Status 'ping'
    $pingResult = Invoke-PingRaw -Protocol $proto -HostName $h -Count $Settings.PingCount -ArgBuilder4 $round.PingArgs4 -ArgBuilder6 $round.PingArgs6 -TimeoutSec (Get-NativeToolTimeoutSec -Tool Ping -Settings $Settings -Round $round)
    Write-Progress -Id $progressId -ParentId 1 -Activity $activity -Status 'tracert'
    $trResult = Invoke-TracertRaw -Protocol $proto -HostName $h -ArgBuilder $round.TracertArgs -TimeoutSec (Get-NativeToolTimeoutSec -Tool Tracert -Settings $Settings -Round $round)

    $ppResult = if ($Settings.SkipPathping) {
      [pscustomobject]@{ Raw = @('Pathping skipped'); ExitCode = $null; TimedOut = $false; TimeoutSec = 0 }
    } else {
      Write-Progress -Id $progressId -ParentId 1 -Activity $activity -Status 'pathping'
      Invoke-PathpingRaw -Protocol $proto -HostName $h -ArgBuilder $round.PathpingArgs -TimeoutSec (Get-NativeToolTimeoutSec -Tool Pathping -Settings $Settings -Round $round)
    }

    Write-Progress -Id $progressId -ParentId 1 -Activity $activity -Status 'tcp 443'
    $tnc = Test-TcpPort -HostName $h -Port 443 -Protocol $proto
  } finally {
    Write-Progress -Id $progressId -Activity $activity -Completed
  }

  $pingOutcome = Get-NativeStageOutcome -Result $pingResult
  # Native tracert/pathping exit with 0 even when the destination is unreachable, so judge by the parsed output.
  $trOutcome = Get-NativeStageOutcome -Result $trResult -TraceDestination $h
  $ppOutcome = if ($Settings.SkipPathping) { $null } else { Get-NativeStageOutcome -Result $ppResult -TraceDestination $h }

  $pingOk = $pingOutcome.Ok
  $tracertOk = $trOutcome.Ok
  $pathpingOk = if ($Settings.SkipPathping) { $null } else { $ppOutcome.Ok }
  $tcp443Ok = [bool]$tnc.TcpTestSucceeded
  $dnsErrorProperty = $tnc.PSObject.Properties['DnsError']
  $dnsError = if ($dnsErrorProperty) { $dnsErrorProperty.Value } else { $null }
  $tcp443Reason = if ($tcp443Ok) { '' } elseif ($dnsError) { "$dnsError" } else { 'TCP connection to port 443 failed' }

  $failedStages = New-Object System.Collections.Generic.List[string]
  $reasons = New-Object System.Collections.Generic.List[string]
  if (-not $pingOk) { $failedStages.Add('Ping') | Out-Null; $reasons.Add("Ping: $($pingOutcome.Reason)") | Out-Null }
  if (-not $tracertOk) { $failedStages.Add('Tracert') | Out-Null; $reasons.Add("Tracert: $($trOutcome.Reason)") | Out-Null }
  if ($null -ne $pathpingOk -and -not $pathpingOk) { $failedStages.Add('Pathping') | Out-Null; $reasons.Add("Pathping: $($ppOutcome.Reason)") | Out-Null }
  if (-not $tcp443Ok) { $failedStages.Add('Tcp443') | Out-Null; $reasons.Add("Tcp443: $tcp443Reason") | Out-Null }
  $overallStatus = if ($failedStages.Count -gt 0) { 'Fail' } else { 'OK' }

  return [pscustomobject]@{
    Timestamp = (Get-Date).ToString('o')
    Round = $round.Name
    Protocol = $proto
    Host = $h
    PingRaw = $pingResult.Raw
    PingOk = $pingOk
    PingStatus = if ($pingOk) { 'OK' } else { 'Fail' }
    TracertRaw = $trResult.Raw
    TracertOk = $tracertOk
    TracertStatus = if ($tracertOk) { 'OK' } else { 'Fail' }
    PathpingRaw = $ppResult.Raw
    PathpingOk = $pathpingOk
    PathpingStatus = if ($Settings.SkipPathping) { 'Skipped' } elseif ($pathpingOk) { 'OK' } else { 'Fail' }
    Tcp443OK = $tcp443Ok
    Tcp443Status = if ($tcp443Ok) { 'OK' } else { 'Fail' }
    TraceRoute = $trResult.Raw
    Ports = @()
    PortsStatus = 'Skipped'
    FailedStages = $failedStages.ToArray()
    FailureReasons = $reasons.ToArray()
    OverallStatus = $overallStatus
  }
}
