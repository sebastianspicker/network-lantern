function Invoke-HostDiagnostics {
  <#
  .SYNOPSIS
    Run the full diagnostic suite (ping, tracert, pathping, TCP/UDP port tests) for one plan item.
  .PARAMETER PlanItem
    A single plan entry from Get-DiagnosticPlan.
  .PARAMETER PortTargets
    Array of @{Name; Protocol; Port} hashtables defining extra port probes.
  .OUTPUTS
    [pscustomobject] Consolidated result with ping/tracert/pathping/TCP/port findings.
  #>
  param(
    [Parameter(Mandatory)][object]$PlanItem,
    [Parameter(Mandatory)][array]$PortTargets,
    [Parameter(Mandatory)][object]$Settings
  )

  $round = $PlanItem.RoundDef
  $proto = $PlanItem.Protocol
  $h = $PlanItem.Host

  $pingResult = Invoke-PingRaw -Protocol $proto -HostName $h -Count $Settings.PingCount -ArgBuilder4 $round.PingArgs4 -ArgBuilder6 $round.PingArgs6
  $trResult = Invoke-TracertRaw -Protocol $proto -HostName $h -ArgBuilder $round.TracertArgs

  $ppResult = if ($Settings.SkipPathping) {
    [pscustomobject]@{ Raw = @('Pathping skipped'); ExitCode = $null }
  } else {
    Invoke-PathpingRaw -Protocol $proto -HostName $h -ArgBuilder $round.PathpingArgs
  }

  $tnc = Test-TcpPort -HostName $h -Port 443 -Hops 20 -Protocol $proto

  $portFindings = @(foreach ($t in $PortTargets) {
    if ($t.Protocol -eq 'TCP') {
      $tcp = Test-TcpPort -HostName $h -Port $t.Port -Hops 10 -Protocol $proto
      $tcpOpen = [bool]$tcp.TcpTestSucceeded
      [pscustomobject]@{
        Name = $t.Name
        Protocol = 'TCP'
        Port = $t.Port
        Success = $tcpOpen
        PathReachable = $tcpOpen
        ServiceOpen = $tcpOpen
        ServiceStatus = if ($tcpOpen) { 'Open' } else { 'ClosedOrFiltered' }
        Note = 'Test-NetConnection'
      }
    } else {
      $udp = Test-UdpPortBestEffort -HostName $h -Port $t.Port -Protocol $proto -TimeoutMs 2000
      $udpReachable = if ($udp.Status -in @('Likely reachable', 'Path OK, port closed')) { $true } else { $false }
      $udpOpen = ($udp.Status -eq 'Likely reachable')
      $udpServiceStatus = switch ($udp.Status) {
        'Likely reachable' { 'OpenOrResponsive' }
        'Path OK, port closed' { 'Closed' }
        'Likely filtered' { 'FilteredOrNoResponse' }
        default { 'Unknown' }
      }
      [pscustomobject]@{
        Name = $t.Name
        Protocol = 'UDP'
        Port = $t.Port
        Success = $udpOpen
        PathReachable = $udpReachable
        ServiceOpen = $udpOpen
        ServiceStatus = $udpServiceStatus
        Note = "$($udp.Status): $($udp.Detail)"
      }
    }
  })

  $pingOk = ($pingResult.ExitCode -eq 0)
  $tracertOk = ($trResult.ExitCode -eq 0)
  $pathpingOk = if ($Settings.SkipPathping) { $null } else { ($ppResult.ExitCode -eq 0) }
  $tcp443Ok = [bool]$tnc.TcpTestSucceeded
  $hasPortFailures = @($portFindings | Where-Object { -not $_.Success }).Count -gt 0

  $failedStages = New-Object System.Collections.Generic.List[string]
  if (-not $pingOk) { $failedStages.Add('Ping') | Out-Null }
  if (-not $tracertOk) { $failedStages.Add('Tracert') | Out-Null }
  if ($null -ne $pathpingOk -and -not $pathpingOk) { $failedStages.Add('Pathping') | Out-Null }
  if (-not $tcp443Ok) { $failedStages.Add('Tcp443') | Out-Null }
  if ($hasPortFailures) { $failedStages.Add('PortProbes') | Out-Null }

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
    TraceRoute = $tnc.TraceRoute
    Ports = $portFindings
    PortsStatus = if ($hasPortFailures) { 'Fail' } else { 'OK' }
    FailedStages = $failedStages.ToArray()
    OverallStatus = $overallStatus
  }
}
