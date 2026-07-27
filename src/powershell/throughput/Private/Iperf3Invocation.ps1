# iperf3 capability probing, invocation, and metric extraction (private to NetworkLantern.Throughput)

function Get-Iperf3Capability {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [ValidateRange(1, 60000)]
    [int]$TimeoutMs = 5000,
    [string]$Iperf3Path
  )
  if ([string]::IsNullOrWhiteSpace($Iperf3Path)) {
    $Iperf3Path = (Get-Command iperf3 -ErrorAction Stop).Source
  }
  $probe = Invoke-Iperf3NativeProcess -FilePath $Iperf3Path -Arguments @('--version') -TimeoutMs $TimeoutMs
  $capExit = $probe.ExitCode
  if ($probe.PSObject.Properties.Name -contains 'Cancelled' -and $probe.Cancelled) {
    throw (New-Iperf3CancellationException -NativeProcess $probe -RunId $probe.CancellationRunId)
  }
  if ($probe.TimedOut) {
    $streamsClosed = -not ($probe.PSObject.Properties.Name -contains 'StreamsCompleted') -or $probe.StreamsCompleted
    if ($probe.TerminationSucceeded -and $streamsClosed) {
      throw [System.TimeoutException]::new("iperf3 --version timed out after $TimeoutMs ms; its tracked process tree was terminated.")
    }
    throw [System.InvalidOperationException]::new("iperf3 --version timed out after $TimeoutMs ms; cleanup was not verified for process $($probe.ProcessId): $($probe.TerminationError) $($probe.StreamReadError)")
  }
  if ($probe.PSObject.Properties.Name -contains 'StreamsCompleted' -and -not $probe.StreamsCompleted) {
    throw [System.IO.InvalidDataException]::new("iperf3 --version output was incomplete: $($probe.StreamReadError)")
  }
  $verOutput = @($probe.StdOut, $probe.StdErr) -join [Environment]::NewLine
  $firstLine = @($verOutput -split '\r?\n' | Where-Object { $_ }) | Select-Object -First 1
  $verText = [string]$firstLine
  if ($capExit -ne 0) {
    $verText = "iperf3 --version failed (exit code $capExit): $verText"
  }
  $m = [regex]::Match($verText, '\b([0-9]+)\.([0-9]+)\b')
  $bidir = $false
  $maj = $null
  $min = $null
  if ($m.Success) {
    $maj = [int]$m.Groups[1].Value
    $min = [int]$m.Groups[2].Value
    $bidir = ($maj -gt 3) -or ($maj -eq 3 -and $min -ge 7)
  }
  [pscustomobject]@{
    VersionText    = [string]$verText
    Major          = $maj
    Minor          = $min
    BidirSupported = $bidir
  }
}

function Invoke-Iperf3 {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]
    [string]$Server,
    [Parameter(Mandatory)]
    [ValidateRange(1, 65535)]
    [int]$Port,
    [Parameter(Mandatory)]
    [ValidateSet('IPv4', 'IPv6')]
    [string]$Stack,
    [Parameter(Mandatory)]
    [ValidateRange(1, 3600)]
    [int]$Duration,
    [Parameter(Mandatory)]
    [ValidateRange(0, 60)]
    [int]$Omit,
    [int]$Tos = 0,
    [Parameter(Mandatory)]
    [ValidateSet('TCP', 'UDP')]
    [string]$Proto,
    [Parameter(Mandatory)]
    [ValidateSet('TX', 'RX', 'BD')]
    [string]$Dir,
    [ValidateRange(1, 128)]
    [int]$Streams = 1,
    [ValidateNotNullOrEmpty()]
    [string]$Win = 'default',
    [ValidateNotNullOrEmpty()]
    [string]$UdpBw = '1M',
    [ValidateRange(1000, 300000)]
    [int]$ConnectTimeoutMs = 60000,
    [Parameter(Mandatory)]
    [pscustomobject]$Caps,
    [scriptblock]$Runner,
    [string]$Iperf3Path,
    [AllowEmptyCollection()]
    [string[]]$NativeArgumentsOverride
  )
  if (-not (Test-ValidHostnameOrIP -Name $Server)) {
    throw "Invalid Server: '$Server'. Must be a valid hostname or IP address."
  }
  $iperfArgs = @('-c', $Server, '-p', $Port, '-t', $Duration, '-O', $Omit, '-J', '--connect-timeout', $ConnectTimeoutMs)
  if ($Stack -eq 'IPv6') { $iperfArgs += '-6' }
  if ($Tos -gt 0) { $iperfArgs += @('-S', $Tos) }
  $udpBwStr = $UdpBw
  if ($udpBwStr -notmatch '[kKmMgG]$') { $udpBwStr = "${udpBwStr}M" }
  if ($Proto -eq 'TCP') {
    if ($Dir -eq 'RX') { $iperfArgs += '-R' }
    if ($Dir -eq 'BD') {
      if ($Caps.BidirSupported) { $iperfArgs += '--bidir' }
      else { throw "iperf3 does not support --bidir in this version: $($Caps.VersionText)" }
    }
    if ($Streams -gt 1) { $iperfArgs += @('-P', $Streams) }
    if ($Win -ne 'default') { $iperfArgs += @('-w', $Win) }
  }
  else {
    $iperfArgs += @('-u', '-b', $udpBwStr)
    if ($Dir -eq 'RX') { $iperfArgs += '-R' }
    if ($Dir -eq 'BD') {
      if ($Caps.BidirSupported) { $iperfArgs += '--bidir' }
      else { throw "iperf3 does not support --bidir in this version: $($Caps.VersionText)" }
    }
  }
  $testStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
  $jsonText = $null
  $nativeProcess = $null
  if ($null -ne $Runner) {
    $rawLines = & $Runner -IperfArgs $iperfArgs 2>&1
    $exitCode = $LASTEXITCODE
  }
  else {
    # Run iperf3 with a timeout to prevent indefinite hangs if the server becomes unresponsive.
    $timeoutMs = ($Duration + $Omit + $script:Iperf3ProcessTimeoutBufferSec) * 1000
    if ([string]::IsNullOrWhiteSpace($Iperf3Path)) {
      $Iperf3Path = (Get-Command iperf3 -ErrorAction Stop).Source
    }
    $nativeArguments = if ($null -ne $NativeArgumentsOverride) { $NativeArgumentsOverride } else { $iperfArgs }
    $nativeProcess = Invoke-Iperf3NativeProcess -FilePath $Iperf3Path -Arguments $nativeArguments -TimeoutMs $timeoutMs
    if ($nativeProcess.Cancelled) {
      throw (New-Iperf3CancellationException -NativeProcess $nativeProcess -RunId $nativeProcess.CancellationRunId)
    }
    if ($nativeProcess.TimedOut) {
      if ($nativeProcess.TerminationSucceeded) {
        Write-Warning "iperf3 process timed out after $([int]($timeoutMs / 1000))s and was terminated."
      }
      else {
        Write-Warning "iperf3 process timed out after $([int]($timeoutMs / 1000))s but could not be terminated. Process $($nativeProcess.ProcessId) may still be running: $($nativeProcess.TerminationError)"
      }
    }
    $exitCode = $nativeProcess.ExitCode
    $stdout = $nativeProcess.StdOut
    $stderr = $nativeProcess.StdErr
    if (-not $nativeProcess.StreamsCompleted) {
      Write-Warning "iperf3 output collection was incomplete: $($nativeProcess.StreamReadError)"
      if ($exitCode -eq 0) { $exitCode = -1 }
    }
    # Try stdout-only for JSON extraction first; fall back to combined stdout+stderr if no JSON found.
    $stdoutLines = @($stdout -split '\r?\n')
    $stdoutText = $stdoutLines -join [Environment]::NewLine
    $jsonText = Get-JsonSubstringOrNull -Text $stdoutText
    if ($jsonText) {
      $rawLines = $stdoutLines
    } else {
      $rawLines = @(($stdout + $stderr) -split '\r?\n')
    }
  }
  $testStopwatch.Stop()
  $durationMs = [int]$testStopwatch.ElapsedMilliseconds
  $rawText = $rawLines -join [Environment]::NewLine
  $jsonObj = $null
  $jsonParseError = $null
  if (-not $jsonText) { $jsonText = Get-JsonSubstringOrNull -Text $rawText }
  if ($null -ne $jsonText) {
    try { $jsonObj = ConvertFrom-Json -InputObject $jsonText }
    catch { Write-Verbose "JSON parse failed: $($_.Exception.Message)"; $jsonObj = $null; $jsonParseError = $_.Exception.Message }
  } elseif ($exitCode -eq 0) {
    $jsonParseError = 'iperf3 JSON output was not found.'
  }
  return [pscustomobject]@{
    Args           = $iperfArgs
    ExitCode       = $exitCode
    RawLines       = $rawLines
    RawText        = $rawText
    Json           = $jsonObj
    JsonParseError = $jsonParseError
    DurationMs     = $durationMs
    ProcessTimedOut = [bool]($nativeProcess -and $nativeProcess.TimedOut)
    Cancelled       = [bool]($nativeProcess -and $nativeProcess.Cancelled)
    TerminationFailed = [bool]($nativeProcess -and $nativeProcess.TimedOut -and -not $nativeProcess.TerminationSucceeded)
    StreamsCompleted = [bool](-not $nativeProcess -or $nativeProcess.StreamsCompleted)
    StreamReadError = if ($nativeProcess) { $nativeProcess.StreamReadError } else { $null }
    TreeTerminationVerified = [bool]($nativeProcess -and $nativeProcess.TreeTerminationVerified)
    TerminationScope = if ($nativeProcess) { $nativeProcess.TerminationScope } else { $null }
    UnterminatedProcessIds = if ($nativeProcess) { [int[]]@($nativeProcess.UnterminatedProcessIds) } else { [int[]]@() }
    ProcessId      = if ($nativeProcess) { $nativeProcess.ProcessId } else { $null }
    TerminationError = if ($nativeProcess) { $nativeProcess.TerminationError } else { $null }
  }
}

function Get-Iperf3Metric {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]
    [object]$Json,
    [Parameter(Mandatory)]
    [ValidateSet('TCP', 'UDP')]
    [string]$Proto,
    [Parameter(Mandatory)]
    [ValidateSet('TX', 'RX', 'BD')]
    [string]$Dir
  )
  if (-not $Json) { return New-Iperf3Metric }
  $end = $Json.end
  if (-not $end) { return New-Iperf3Metric }
  $sumSent = $null
  $sumRecv = $null
  $sumUdp = $null
  if ($end.PSObject.Properties.Name -contains 'sum_sent') { $sumSent = $end.sum_sent }
  if ($end.PSObject.Properties.Name -contains 'sum_received') { $sumRecv = $end.sum_received }
  if ($end.PSObject.Properties.Name -contains 'sum') { $sumUdp = $end.sum }
  $txMbps = $null
  $rxMbps = $null
  $retr = $null
  $loss = $null
  $jit = $null
  if ($Proto -eq 'TCP') {
    if ($Dir -eq 'TX' -or $Dir -eq 'BD') {
      $txMbps = Get-BitsPerSecondMbps -Obj $sumSent
      $rxMbps = Get-BitsPerSecondMbps -Obj $sumRecv
    }
    elseif ($Dir -eq 'RX') {
      $rxMbps = Get-BitsPerSecondMbps -Obj $sumRecv
      $txMbps = Get-BitsPerSecondMbps -Obj $sumSent
    }
    if ($sumSent -and ($sumSent.PSObject.Properties.Name -contains 'retransmits')) { $retr = [int]$sumSent.retransmits }
    return [pscustomobject]@{ TxMbps = $txMbps; RxMbps = $rxMbps; Retr = $retr; LossPct = $null; JitterMs = $null }
  }
  # UDP: support end.sum when sum_sent/sum_received missing (common in iperf3 JSON)
  $sentBps = $sumSent
  $recvBps = $sumRecv
  if (-not $sentBps -and $sumUdp -and ($sumUdp.PSObject.Properties.Name -contains 'bits_per_second')) { $sentBps = $sumUdp }
  if (-not $recvBps -and $sumUdp) { $recvBps = $sumUdp }
  if ($Dir -eq 'TX') {
    $txMbps = Get-BitsPerSecondMbps -Obj $sentBps
    $rxMbps = Get-BitsPerSecondMbps -Obj $recvBps
  }
  elseif ($Dir -eq 'RX') {
    $txMbps = Get-BitsPerSecondMbps -Obj $sentBps
    $rxMbps = Get-BitsPerSecondMbps -Obj $recvBps
  }
  else {
    $txMbps = Get-BitsPerSecondMbps -Obj $sentBps
    $rxMbps = Get-BitsPerSecondMbps -Obj $recvBps
  }
  if ($sumSent -and ($sumSent.PSObject.Properties.Name -contains 'lost_percent')) { $loss = [double]$sumSent.lost_percent }
  elseif ($sumUdp -and ($sumUdp.PSObject.Properties.Name -contains 'lost_percent')) { $loss = [double]$sumUdp.lost_percent }
  if ($sumUdp -and ($sumUdp.PSObject.Properties.Name -contains 'jitter_ms')) { $jit = [double]$sumUdp.jitter_ms }
  return [pscustomobject]@{ TxMbps = $txMbps; RxMbps = $rxMbps; Retr = $null; LossPct = $loss; JitterMs = $jit }
}
