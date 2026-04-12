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
    [Parameter(Mandatory)][array]$PortTargets
  )

  $round = $PlanItem.RoundDef
  $proto = $PlanItem.Protocol
  $h = $PlanItem.Host

  $pingResult = Invoke-PingRaw -Protocol $proto -HostName $h -Count $PingCount -ArgBuilder4 $round.PingArgs4 -ArgBuilder6 $round.PingArgs6
  $trResult = Invoke-TracertRaw -Protocol $proto -HostName $h -ArgBuilder $round.TracertArgs

  $ppResult = if ($SkipPathping) {
    [pscustomobject]@{ Raw = @('Pathping skipped'); ExitCode = 0 }
  } else {
    Invoke-PathpingRaw -Protocol $proto -HostName $h -ArgBuilder $round.PathpingArgs
  }

  $tnc = Test-TcpPort -HostName $h -Port 443 -Hops 20 -Protocol $proto

  $portFindings = @(foreach ($t in $PortTargets) {
    if ($t.Protocol -eq 'TCP') {
      $tcp = Test-TcpPort -HostName $h -Port $t.Port -Hops 10 -Protocol $proto
      [pscustomobject]@{
        Name = $t.Name
        Protocol = 'TCP'
        Port = $t.Port
        Success = [bool]$tcp.TcpTestSucceeded
        Note = 'Test-NetConnection'
      }
    } else {
      $udp = Test-UdpPortBestEffort -HostName $h -Port $t.Port -Protocol $proto -TimeoutMs 2000
      [pscustomobject]@{
        Name = $t.Name
        Protocol = 'UDP'
        Port = $t.Port
        Success = ($udp.Status -eq 'Likely reachable')
        Note = "$($udp.Status): $($udp.Detail)"
      }
    }
  })

  return [pscustomobject]@{
    Timestamp = (Get-Date).ToString('o')
    Round = $round.Name
    Protocol = $proto
    Host = $h
    PingRaw = $pingResult.Raw
    PingOk = ($pingResult.ExitCode -eq 0)
    TracertRaw = $trResult.Raw
    TracertOk = ($trResult.ExitCode -eq 0)
    PathpingRaw = $ppResult.Raw
    PathpingOk = ($ppResult.ExitCode -eq 0)
    Tcp443OK = [bool]$tnc.TcpTestSucceeded
    TraceRoute = $tnc.TraceRoute
    Ports = $portFindings
  }
}
