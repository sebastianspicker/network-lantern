function Test-UdpPortBestEffort {
  <#
  .SYNOPSIS
    Best-effort UDP reachability probe (send empty datagram, interpret response).
  .PARAMETER HostName
    Target host.
  .PARAMETER Port
    UDP port number.
  .PARAMETER Protocol
    IPv4 or IPv6.
  .PARAMETER TimeoutMs
    Socket receive timeout in milliseconds (default 2000).
  .PARAMETER DnsTimeoutMs
    DNS resolution timeout in milliseconds (default 5000).
  .OUTPUTS
    [pscustomobject] with Host, Port, Protocol, Status, Detail, DurationMs.
  #>
  param(
    [Parameter(Mandatory)][string]$HostName,
    [Parameter(Mandatory)][int]$Port,
    [Parameter(Mandatory)][ValidateSet('IPv4', 'IPv6')]$Protocol,
    [int]$TimeoutMs = 2000,
    [int]$DnsTimeoutMs = 5000
  )

  $result = [ordered]@{
    Host       = $HostName
    Port       = $Port
    Protocol   = 'UDP'
    Status     = 'Ambiguous'
    Detail     = ''
    DurationMs = 0
  }

  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  $udp = $null
  try {
    $family = if ($Protocol -eq 'IPv6') { [System.Net.Sockets.AddressFamily]::InterNetworkV6 } else { [System.Net.Sockets.AddressFamily]::InterNetwork }
    $addrs, $dnsError = Get-HostAddressesWithTimeout -HostName $HostName -TimeoutMs $DnsTimeoutMs
    if ($dnsError) {
      $result.Detail = $dnsError
      return [pscustomobject]$result
    }

    $addr = $addrs | Where-Object { $_.AddressFamily -eq $family } | Select-Object -First 1
    if (-not $addr) {
      $result.Detail = "No $Protocol address for $HostName"
      return [pscustomobject]$result
    }

    $udp = [System.Net.Sockets.UdpClient]::new($family)
    $udp.Client.ReceiveTimeout = $TimeoutMs
    $udp.Connect($addr, $Port)

    $bytes = [byte[]]::new(0)
    [void]$udp.Send($bytes, 0)

    $anyAddr = if ($Protocol -eq 'IPv6') { [System.Net.IPAddress]::IPv6Any } else { [System.Net.IPAddress]::Any }
    $remote = New-Object System.Net.IPEndPoint($anyAddr, 0)
    try {
      [void]$udp.Receive([ref]$remote)
      $result.Status = 'Likely reachable'
      $result.Detail = 'Received datagram response'
    } catch [System.Net.Sockets.SocketException] {
      switch ($_.Exception.ErrorCode) {
        10054 { $result.Status = 'Path OK, port closed'; $result.Detail = 'ICMP Port Unreachable received' }
        10060 { $result.Status = 'Likely filtered'; $result.Detail = 'Receive timeout' }
        default { $result.Status = 'Likely filtered'; $result.Detail = "Socket error $($_.Exception.ErrorCode)" }
      }
    }
  }
  catch {
    $result.Status = 'Error'
    $result.Detail = $_.Exception.Message
  }
  finally {
    if ($udp) {
      try { $udp.Close() } catch { $null = $_ }
    }
    $sw.Stop()
    $result.DurationMs = [int]$sw.Elapsed.TotalMilliseconds
  }

  return [pscustomobject]$result
}
