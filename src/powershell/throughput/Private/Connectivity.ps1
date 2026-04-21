# Reachability, TCP port, MTU probe, and test-suite connectivity (private to Iperf3TestSuite)

function Test-Reachability {
  [CmdletBinding()]
  [OutputType([string])]
  param(
    [Parameter(Mandatory)]
    [string]$ComputerName,
    [Parameter(Mandatory)]
    [ValidateSet('Auto', 'IPv4', 'IPv6')]
    [string]$Mode
  )
  if (-not $IsWindows) {
    throw "Test-Reachability is currently only supported on Windows due to dependency on ping.exe or Test-Connection behaviors."
  }
  $stacksToTry = switch ($Mode) {
    'IPv4' { @('IPv4') }
    'IPv6' { @('IPv6') }
    default { @('IPv4', 'IPv6') }
  }
  if (-not (Test-ValidHostnameOrIP -Name $ComputerName)) {
    throw "Invalid ComputerName: '$ComputerName'. Must be a valid hostname or IP address."
  }
  foreach ($stack in $stacksToTry) {
    try {
      if ($stack -eq 'IPv4' -and (Test-Connection -ComputerName $ComputerName -Count 1 -Quiet -IPv4 -ErrorAction Stop)) { return 'IPv4' }
      if ($stack -eq 'IPv6' -and (Test-Connection -ComputerName $ComputerName -Count 1 -Quiet -IPv6 -ErrorAction Stop)) { return 'IPv6' }
    }
    catch {
      try {
        $pingArgs = Get-PingArgumentsForStack -Stack $stack -ComputerName $ComputerName
        $null = & ping.exe @pingArgs 2>$null
        if ($LASTEXITCODE -eq 0) { return $stack }
      }
      catch { Write-Verbose "ping.exe failed for $stack; continuing to next stack." }
    }
  }
  return 'None'
}

function Test-TcpPortAndTrace {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]
    [string]$ComputerName,
    [Parameter(Mandatory)]
    [ValidateRange(1, 65535)]
    [int]$Port,
    [ValidateRange(1, 30)]
    [int]$Hops = $script:DefaultTraceHops,
    [ValidateRange(1000, 300000)]
    [int]$TimeoutMs = 10000
  )
  $tcp = $null
  $trace = $null
  if ($IsWindows) {
    try { $tcp = Test-NetConnection -ComputerName $ComputerName -Port $Port -InformationLevel Detailed -ErrorAction Stop }
    catch { Write-Warning "TCP port $Port check on '$ComputerName' failed: $($_.Exception.Message)" }
    if ($tcp -and $tcp.TcpTestSucceeded) {
      try { $trace = Test-NetConnection -ComputerName $ComputerName -TraceRoute -Hops $Hops -InformationLevel Detailed -ErrorAction Stop }
      catch { Write-Verbose "Traceroute failed (e.g. ICMP filtered); TCP result still valid." }
    }
  }
  else {
    # Cross-platform fallback: use TcpClient for port check; traceroute unavailable.
    $client = New-Object System.Net.Sockets.TcpClient
    try {
      $task = $client.ConnectAsync($ComputerName, $Port)
      $connected = $task.Wait($TimeoutMs)
      if ($task.IsFaulted) { $null = $task.Exception }
      $tcp = [pscustomobject]@{
        TcpTestSucceeded = ($connected -and $client.Connected)
        RemoteAddress    = $ComputerName
        PingSucceeded    = $null
      }
    }
    catch {
      Write-Warning "TCP port $Port check on '$ComputerName' failed: $($_.Exception.Message)"
      $tcp = [pscustomobject]@{ TcpTestSucceeded = $false; RemoteAddress = $ComputerName; PingSucceeded = $null }
    }
    finally {
      if ($client) { $client.Dispose() }
    }
    Write-Verbose "Traceroute not available on non-Windows platforms."
  }
  [pscustomobject]@{ Tcp = $tcp; Trace = $trace }
}

function Test-MtuPayload {
  [CmdletBinding()]
  [OutputType([int[]])]
  param(
    [Parameter(Mandatory)]
    [string]$ComputerName,
    [Parameter(Mandatory)]
    [ValidateSet('IPv4', 'IPv6')]
    [string]$Stack,
    [Parameter(Mandatory)]
    [int[]]$Sizes
  )
  if (-not $IsWindows) {
    throw "Test-MtuPayload is currently only supported on Windows due to dependency on ping.exe."
  }
  if (-not (Test-ValidHostnameOrIP -Name $ComputerName)) {
    throw "Invalid ComputerName: '$ComputerName'. Must be a valid hostname or IP address."
  }
  $fails = New-Object System.Collections.Generic.List[int]
  foreach ($sz in $Sizes) {
    $pingArgs = Get-PingArgumentsForStack -Stack $Stack -ComputerName $ComputerName -MtuPayloadSize $sz
    $null = & ping.exe @pingArgs 2>$null
    if ($LASTEXITCODE -ne 0) { [void]$fails.Add($sz) }
  }
  return $fails.ToArray()
}

function Test-Iperf3TestSuitePrerequisites {
  [CmdletBinding()]
  [OutputType([void])]
  param(
    [switch]$SkipReachabilityCheck,
    [switch]$DisableMtuProbe
  )
  $null = Get-Command iperf3 -ErrorAction Stop
  $null = Get-Command ConvertFrom-Json -ErrorAction Stop
  if ((-not ($SkipReachabilityCheck -and $DisableMtuProbe)) -and $IsWindows) {
    $pingCmd = Get-Command ping.exe -ErrorAction SilentlyContinue
    if (-not $pingCmd) {
      throw "ping.exe is required for reachability check or MTU probe but was not found. Use -SkipReachabilityCheck and -DisableMtuProbe to run without it (Windows only)."
    }
  }
  if (-not $IsWindows -and -not ($SkipReachabilityCheck -and $DisableMtuProbe)) {
    throw "Reachability check and MTU probe require Windows (ping.exe, Test-NetConnection). Use -SkipReachabilityCheck -DisableMtuProbe to run on Linux/macOS."
  }
}

function Get-TestSuiteConnectivity {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]
    [string]$Target,
    [Parameter(Mandatory)]
    [int]$Port,
    [Parameter(Mandatory)]
    [string]$IpVersion,
    [switch]$SkipReachabilityCheck,
    [switch]$DisableMtuProbe,
    [Parameter(Mandatory)]
    [int[]]$MtuSizes,
    [int]$ConnectTimeoutMs = 10000
  )
  $stack = 'None'
  if ($SkipReachabilityCheck) {
    Write-Verbose "Reachability check skipped (-SkipReachabilityCheck); proceeding with TCP port check only."
  }
  else {
    $stack = Test-Reachability -ComputerName $Target -Mode $IpVersion
    if ($stack -eq 'None') {
      throw "ICMP reachability to '$Target' failed; aborting. Use -SkipReachabilityCheck to proceed when only TCP is reachable."
    }
  }
  $net = Test-TcpPortAndTrace -ComputerName $Target -Port $Port -Hops $script:DefaultTraceHops -TimeoutMs $ConnectTimeoutMs
  if (-not $net -or -not $net.Tcp -or -not $net.Tcp.TcpTestSucceeded) {
    throw "TCP port $Port on '$Target' not reachable. Verify the iperf3 server is running (iperf3 -s -p $Port) and the port is not blocked by a firewall."
  }
  if ($stack -eq 'None') {
    $stack = if ($net.Tcp.RemoteAddress -match ':') { 'IPv6' } else { 'IPv4' }
    Write-Verbose "Using stack $stack from TCP connection."
  }
  $mtuFails = @()
  if (-not $DisableMtuProbe) {
    $mtuFails = Test-MtuPayload -ComputerName $Target -Stack $stack -Sizes $MtuSizes
  }
  return [pscustomobject]@{ Stack = $stack; Net = $net; MtuFails = $mtuFails }
}
