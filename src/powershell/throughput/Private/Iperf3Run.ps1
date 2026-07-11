# iperf3 capability, invoke, metric extraction, single-test runner (private to Iperf3TestSuite)

function Get-Iperf3Capability {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param()
  $verOutput = & iperf3 --version 2>&1
  $capExit = $LASTEXITCODE
  $firstLine = $verOutput | Select-Object -First 1
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
    [scriptblock]$Runner
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
  if ($null -ne $Runner) {
    $rawLines = & $Runner -IperfArgs $iperfArgs 2>&1
    $exitCode = $LASTEXITCODE
  }
  else {
    # Run iperf3 with a timeout to prevent indefinite hangs if the server becomes unresponsive.
    $timeoutMs = ($Duration + $Omit + $script:Iperf3ProcessTimeoutBufferSec) * 1000
    $iperf3Path = (Get-Command iperf3 -ErrorAction Stop).Source
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $iperf3Path
    # Use ArgumentList.AddRange (available in .NET 5+/PowerShell 7+) to avoid string-parsing issues.
    $iperfArgs | ForEach-Object { $psi.ArgumentList.Add([string]$_) }
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $proc = [System.Diagnostics.Process]::Start($psi)
    # Read stderr asynchronously first to avoid deadlock when both buffers fill.
    $stderrTask = $proc.StandardError.ReadToEndAsync()
    $stdout = $proc.StandardOutput.ReadToEnd()
    $stderr = $stderrTask.GetAwaiter().GetResult()
    $completed = $proc.WaitForExit($timeoutMs)
    if (-not $completed) {
      try { $proc.Kill() } catch { Write-Verbose "Failed to kill timed-out iperf3 process: $($_.Exception.Message)" }
      $proc.WaitForExit(5000)
      Write-Warning "iperf3 process timed out after $([int]($timeoutMs / 1000))s and was terminated."
    }
    if ($proc.HasExited) { $exitCode = $proc.ExitCode } else { $exitCode = -1; Write-Warning "Process did not exit after kill" }
    # Try stdout-only for JSON extraction first; fall back to combined stdout+stderr if no JSON found.
    $stdoutLines = @($stdout -split '\r?\n')
    $stdoutText = $stdoutLines -join [Environment]::NewLine
    $jsonText = Get-JsonSubstringOrNull -Text $stdoutText
    if ($jsonText) {
      $rawLines = $stdoutLines
    } else {
      $rawLines = @(($stdout + $stderr) -split '\r?\n')
    }
    $proc.Dispose()
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

function Invoke-SingleIperf3TestAndAddResult {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]
    [AllowEmptyCollection()]
    [System.Collections.Generic.List[object]]$AllResultsList,
    [Parameter(Mandatory)]
    [AllowEmptyCollection()]
    [System.Collections.Generic.List[object]]$CsvRowsList,
    [Parameter(Mandatory)]
    [int]$No,
    [Parameter(Mandatory)]
    [ValidateSet('TCP', 'UDP')]
    [string]$Proto,
    [Parameter(Mandatory)]
    [ValidateSet('TX', 'RX', 'BD')]
    [string]$Dir,
    [Parameter(Mandatory)]
    [string]$DSCP,
    [Parameter(Mandatory)]
    [int]$Tos,
    [int]$Streams = 1,
    [string]$Window = '',
    [string]$UdpBw = '',
    [Parameter(Mandatory)]
    [string]$Stack,
    [Parameter(Mandatory)]
    [string]$Target,
    [Parameter(Mandatory)]
    [int]$Port,
    [Parameter(Mandatory)]
    [int]$Duration,
    [Parameter(Mandatory)]
    [int]$Omit,
    [Parameter(Mandatory)]
    [int]$ConnectTimeoutMs,
    [Parameter(Mandatory)]
    [pscustomobject]$Caps
  )
  $retryCount = if ($null -ne $script:ActiveRetryCount) { $script:ActiveRetryCount } else { $script:DefaultRetryCount }
  $invokeParams = @{
    Server           = $Target
    Port             = $Port
    Stack            = $Stack
    Duration         = $Duration
    Omit             = $Omit
    Tos              = $Tos
    Proto            = $Proto
    Dir              = $Dir
    Streams          = $Streams
    Win              = if ($Window) { $Window } else { 'default' }
    ConnectTimeoutMs = $ConnectTimeoutMs
    Caps             = $Caps
  }
  if ($UdpBw) { $invokeParams['UdpBw'] = $UdpBw }
  $run = Invoke-Iperf3 @invokeParams
  # Retry on transient failure: non-zero exit AND no JSON (no partial results to salvage)
  $attempt = 0
  while ($run.ExitCode -ne 0 -and $null -eq $run.Json -and $attempt -lt $retryCount) {
    $attempt++
    Write-Verbose "Test #$No failed (exit $($run.ExitCode), no JSON). Retry $attempt/$retryCount in $($script:RetryDelayMs)ms..."
    Start-Sleep -Milliseconds $script:RetryDelayMs
    $run = Invoke-Iperf3 @invokeParams
  }
  $m = Get-Iperf3Metric -Json $run.Json -Proto $Proto -Dir $Dir
  Add-Iperf3TestResult -AllResultsList $AllResultsList -CsvRowsList $CsvRowsList -No $No -Proto $Proto -Dir $Dir -DSCP $DSCP -Tos $Tos -Streams $Streams -Window $Window -UdpBw $UdpBw -Stack $Stack -Target $Target -Port $Port -Run $run -Metrics $m
  return [pscustomobject]@{ Run = $run; Metrics = $m }
}

# Ramps UDP bandwidth from CurMbps to MaxMbps in StepMbps increments for each direction (TX, RX).
# Stops early when packet loss exceeds UdpLossThreshold or iperf3 fails without JSON output.
# This finds the practical bandwidth ceiling for each DSCP class under the current network conditions.
function Invoke-UdpSaturationForDscp {
  [CmdletBinding()]
  param(
    [Parameter(Mandatory)]
    [System.Collections.Generic.List[object]]$AllResultsList,
    [Parameter(Mandatory)]
    [System.Collections.Generic.List[object]]$CsvRowsList,
    [Parameter(Mandatory)]
    [ref]$TestNoRef,
    [Parameter(Mandatory)]
    [string]$Dscp,
    [Parameter(Mandatory)]
    [int]$Tos,
    [Parameter(Mandatory)]
    [string]$Stack,
    [Parameter(Mandatory)]
    [string]$Target,
    [Parameter(Mandatory)]
    [int]$Port,
    [Parameter(Mandatory)]
    [int]$Duration,
    [Parameter(Mandatory)]
    [int]$Omit,
    [Parameter(Mandatory)]
    [int]$ConnectTimeoutMs,
    [Parameter(Mandatory)]
    [pscustomobject]$Caps,
    [Parameter(Mandatory)]
    [double]$UdpLossThreshold,
    [Parameter(Mandatory)]
    [double]$CurMbps,
    [Parameter(Mandatory)]
    [double]$MaxMbps,
    [Parameter(Mandatory)]
    [double]$StepMbps,
    [switch]$Progress,
    [Parameter(Mandatory)]
    [int]$TotalApprox,
    [int]$MaxUdpIterations = $script:MaxUdpSaturationIterations
  )
  $cur = $CurMbps
  $max = $MaxMbps
  if ($max -lt $cur) { $max = $cur }
  $step = $StepMbps
  foreach ($dir in @('TX', 'RX')) {
    $bw = $cur
    $iterations = 0
    while ($bw -le $max -and $iterations -lt $MaxUdpIterations) {
      $iterations++
      $TestNoRef.Value++
      $testNo = $TestNoRef.Value
      if ($Progress) {
        $pct = [math]::Min(100, [int](100 * $testNo / $TotalApprox))
        Write-Information -InformationAction Continue "Running test $testNo/$TotalApprox ($pct%) (UDP saturation $dir $Dscp $bw Mbit/s)..."
      }
      $bwStr = [string]::Format($script:InvariantCulture, '{0}M', $bw)
      $res = Invoke-SingleIperf3TestAndAddResult -AllResultsList $AllResultsList -CsvRowsList $CsvRowsList -No $testNo -Proto 'UDP' -Dir $dir -DSCP $Dscp -Tos $Tos -Window '' -UdpBw $bwStr -Stack $Stack -Target $Target -Port $Port -Duration $Duration -Omit $Omit -ConnectTimeoutMs $ConnectTimeoutMs -Caps $Caps
      $run = $res.Run
      $m = $res.Metrics
      if ($run.ExitCode -ne 0 -and $null -eq $run.Json) { break }
      if ($null -ne $m.LossPct -and [double]$m.LossPct -gt $UdpLossThreshold) { break }
      $bw += $step
    }
  }
}
