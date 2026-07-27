# iperf3 test execution and UDP saturation helpers (private to NetworkLantern.Throughput)

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
  # Build-TestPlan budgets no saturation work unless the ceiling is above the
  # fixed-rate starting point. Keep execution aligned with that plan so test
  # counts and progress denominators remain exact.
  if ($MaxMbps -le $CurMbps) { return }
  $cur = $CurMbps
  $max = $MaxMbps
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
