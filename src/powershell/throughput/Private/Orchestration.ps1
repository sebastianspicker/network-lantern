# Orchestration helpers for Invoke-Iperf3TestSuite (private to Iperf3TestSuite)

function Build-TestPlan {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]
    [switch]$SingleTest,
    [Parameter(Mandatory)]
    [ValidateSet('TCP', 'UDP', 'Both')]
    [string]$Protocol,
    [Parameter(Mandatory)]
    [string[]]$DscpClasses,
    [Parameter(Mandatory)]
    [int[]]$TcpStreams,
    [Parameter(Mandatory)]
    [string[]]$TcpWindows,
    [Parameter(Mandatory)]
    [pscustomobject]$Caps,
    [Parameter(Mandatory)]
    [string]$UdpStart,
    [Parameter(Mandatory)]
    [string]$UdpMax,
    [Parameter(Mandatory)]
    [string]$UdpStep
  )
  $nDscp = if ($SingleTest) { 1 } else { $DscpClasses.Count }
  $nTcpStreams = if ($SingleTest) { 1 } else { $TcpStreams.Count }
  $nTcpWindows = if ($SingleTest) { 1 } else { $TcpWindows.Count }
  $dirsTcp = if ($Caps.BidirSupported) { 3 } else { 2 }
  $tcpPerDscp = if ($Protocol -eq 'UDP') { 0 } else { $dirsTcp * $nTcpStreams * $nTcpWindows }
  $udpFixedPerDscp = if ($Protocol -eq 'TCP' -or $SingleTest) { 0 } else { 2 }
  $curMbps = ConvertTo-MbitPerSecond $UdpStart
  $maxMbps = ConvertTo-MbitPerSecond $UdpMax
  $stepMbps = [math]::Max((ConvertTo-MbitPerSecond $UdpStep), 1)
  $udpSatSteps = if ($Protocol -eq 'TCP' -or $SingleTest -or $maxMbps -le $curMbps) { 0 } else { [math]::Min([int](($maxMbps - $curMbps) / $stepMbps) + 1, $script:MaxUdpSaturationIterations) }
  $udpSatPerDscp = 2 * $udpSatSteps
  $runSingleUdp = [bool]($SingleTest -and $Protocol -eq 'UDP')
  $runTcp = if ($SingleTest) { $Protocol -ne 'UDP' } else { ($Protocol -eq 'TCP' -or $Protocol -eq 'Both') }
  $runUdpMatrix = if ($SingleTest) { $false } else { ($Protocol -eq 'UDP' -or $Protocol -eq 'Both') }
  $totalApprox = if ($SingleTest) { 1 } else { $nDscp * ($tcpPerDscp + $udpFixedPerDscp + $udpSatPerDscp) }
  if ($totalApprox -lt 1) { $totalApprox = 1 }
  return [pscustomobject]@{
    TotalApprox    = $totalApprox
    CurMbps        = $curMbps
    MaxMbps        = $maxMbps
    StepMbps       = $stepMbps
    DscpList       = if ($SingleTest) { $DscpClasses[0..0] } else { $DscpClasses }
    TcpStreamsList = if ($SingleTest) { @(1) } else { $TcpStreams }
    TcpWindowsList = if ($SingleTest) { @('default') } else { $TcpWindows }
    DirsTcpList    = if ($SingleTest) { @('TX') } else { @('TX', 'RX', 'BD') }
    RunTcp         = $runTcp
    RunUdp         = $runUdpMatrix
    RunSingleUdp   = $runSingleUdp
  }
}

function Invoke-TcpMatrix {
  [CmdletBinding()]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]
    [AllowEmptyCollection()]
    [System.Collections.Generic.List[object]]$AllResultsList,
    [Parameter(Mandatory)]
    [AllowEmptyCollection()]
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
    [string[]]$DirsTcpList,
    [Parameter(Mandatory)]
    [int[]]$TcpStreamsList,
    [Parameter(Mandatory)]
    [string[]]$TcpWindowsList,
    [switch]$Progress,
    [Parameter(Mandatory)]
    [int]$TotalApprox
  )
  foreach ($dir in $DirsTcpList) {
    if ($dir -eq 'BD' -and -not $Caps.BidirSupported) { continue }
    foreach ($s in $TcpStreamsList) {
      foreach ($w in $TcpWindowsList) {
        $TestNoRef.Value++
        $testNo = $TestNoRef.Value
        if ($Progress) {
          $pct = [math]::Min(100, [int](100 * $testNo / $TotalApprox))
          Write-Information -InformationAction Continue "Running test $testNo/$TotalApprox ($pct%) (TCP $dir $Dscp P=$s w=$w)..."
        }
        $null = Invoke-SingleIperf3TestAndAddResult -AllResultsList $AllResultsList -CsvRowsList $CsvRowsList -No $testNo -Proto 'TCP' -Dir $dir -DSCP $Dscp -Tos $Tos -Streams $s -Window $w -UdpBw '' -Stack $Stack -Target $Target -Port $Port -Duration $Duration -Omit $Omit -ConnectTimeoutMs $ConnectTimeoutMs -Caps $Caps
      }
    }
  }
}

function Invoke-UdpMatrix {
  [CmdletBinding()]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]
    [AllowEmptyCollection()]
    [System.Collections.Generic.List[object]]$AllResultsList,
    [Parameter(Mandatory)]
    [AllowEmptyCollection()]
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
    [string]$UdpStart,
    [switch]$Progress,
    [Parameter(Mandatory)]
    [int]$TotalApprox
  )
  foreach ($dir in @('TX', 'RX')) {
    $TestNoRef.Value++
    $testNo = $TestNoRef.Value
    if ($Progress) {
      $pct = [math]::Min(100, [int](100 * $testNo / $TotalApprox))
      Write-Information -InformationAction Continue "Running test $testNo/$TotalApprox ($pct%) (UDP $dir $Dscp)..."
    }
    $null = Invoke-SingleIperf3TestAndAddResult -AllResultsList $AllResultsList -CsvRowsList $CsvRowsList -No $testNo -Proto 'UDP' -Dir $dir -DSCP $Dscp -Tos $Tos -Window '' -UdpBw $UdpStart -Stack $Stack -Target $Target -Port $Port -Duration $Duration -Omit $Omit -ConnectTimeoutMs $ConnectTimeoutMs -Caps $Caps
  }
}

function Invoke-UdpSingleTest {
  [CmdletBinding()]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]
    [AllowEmptyCollection()]
    [System.Collections.Generic.List[object]]$AllResultsList,
    [Parameter(Mandatory)]
    [AllowEmptyCollection()]
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
    [string]$UdpStart,
    [switch]$Progress,
    [Parameter(Mandatory)]
    [int]$TotalApprox
  )
  $TestNoRef.Value++
  $testNo = $TestNoRef.Value
  if ($Progress) {
    $pct = [math]::Min(100, [int](100 * $testNo / $TotalApprox))
    Write-Information -InformationAction Continue "Running test $testNo/$TotalApprox ($pct%) (UDP TX $Dscp)..."
  }
  $null = Invoke-SingleIperf3TestAndAddResult -AllResultsList $AllResultsList -CsvRowsList $CsvRowsList -No $testNo -Proto 'UDP' -Dir 'TX' -DSCP $Dscp -Tos $Tos -Window '' -UdpBw $UdpStart -Stack $Stack -Target $Target -Port $Port -Duration $Duration -Omit $Omit -ConnectTimeoutMs $ConnectTimeoutMs -Caps $Caps
}

function Invoke-UdpSaturationMatrix {
  [CmdletBinding()]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]
    [AllowEmptyCollection()]
    [System.Collections.Generic.List[object]]$AllResultsList,
    [Parameter(Mandatory)]
    [AllowEmptyCollection()]
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
    [int]$TotalApprox
  )
  Invoke-UdpSaturationForDscp -AllResultsList $AllResultsList -CsvRowsList $CsvRowsList -TestNoRef $TestNoRef `
    -Dscp $Dscp -Tos $Tos -Stack $Stack -Target $Target -Port $Port -Duration $Duration -Omit $Omit -ConnectTimeoutMs $ConnectTimeoutMs -Caps $Caps `
    -UdpLossThreshold $UdpLossThreshold -CurMbps $CurMbps -MaxMbps $MaxMbps -StepMbps $StepMbps -Progress:$Progress -TotalApprox $TotalApprox
}

function Write-FinalOutputs {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]
    [AllowEmptyCollection()]
    [System.Collections.Generic.List[object]]$CsvRowsList,
    [Parameter(Mandatory)]
    [AllowEmptyCollection()]
    [System.Collections.Generic.List[object]]$AllResultsList,
    [Parameter(Mandatory)]
    [string]$CsvPath,
    [Parameter(Mandatory)]
    [string]$JsonPath,
    [Parameter(Mandatory)]
    [pscustomobject]$FinalResultObject,
    [Parameter(Mandatory)]
    [string]$OutDir,
    [Parameter(Mandatory)]
    [string]$Timestamp,
    [string]$StartedUtc,
    [string]$CompletedUtc,
    [double]$ElapsedSeconds = 0,
    [string]$Iperf3Version,
    [nullable[double]]$ThresholdMinThroughputMbps,
    [nullable[double]]$ThresholdMaxLossPct,
    [nullable[double]]$ThresholdMaxJitterMs
  )
  try {
    $CsvRowsList | Export-Csv -LiteralPath $CsvPath -NoTypeInformation -Encoding UTF8
    $csvStatus = 'OK'
  }
  catch {
    Write-Warning "Failed to write CSV output to '$CsvPath': $($_.Exception.Message)"
    $CsvPath = $null
    $csvStatus = 'Warn'
  }
  try {
    $FinalResultObject | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $JsonPath -Encoding UTF8
    $jsonStatus = 'OK'
  }
  catch {
    Write-Warning "Failed to write JSON output to '$JsonPath': $($_.Exception.Message)"
    $JsonPath = $null
    $jsonStatus = 'Warn'
  }
  $failedCount = @($AllResultsList | Where-Object {
      $_.ExitCode -ne 0 -or
      $_.JsonParseError -or
      ($_.PSObject.Properties.Name -contains 'MetricError' -and $_.MetricError)
    }).Count
  $parseErrorCount = @($AllResultsList | Where-Object { $_.JsonParseError }).Count
  $runSummary = Build-RunSummary -Results $AllResultsList.ToArray() -TestCount $AllResultsList.Count -ParseErrorCount $parseErrorCount -Target $FinalResultObject.Target -Port $FinalResultObject.Port -Stack $FinalResultObject.Stack -Timestamp $Timestamp -OutDir $OutDir -StartedUtc $StartedUtc -CompletedUtc $CompletedUtc -ElapsedSeconds $ElapsedSeconds -Iperf3Version $Iperf3Version -ThresholdMinThroughputMbps $ThresholdMinThroughputMbps -ThresholdMaxLossPct $ThresholdMaxLossPct -ThresholdMaxJitterMs $ThresholdMaxJitterMs
  $runSummary.ArtifactStatus.Csv = $csvStatus
  $runSummary.ArtifactStatus.Json = $jsonStatus
  $supplemental = Write-Iperf3SupplementalReports -RunSummary $runSummary -OutDir $OutDir -Timestamp $Timestamp
  $runSummary.ArtifactStatus.SummaryJson = $supplemental.SummaryJsonStatus
  $runSummary.ArtifactStatus.ReportMd = $supplemental.ReportMdStatus
  $runIndexPath = Write-Iperf3RunIndex -OutDir $OutDir -RunSummary $runSummary -CsvPath $CsvPath -JsonPath $JsonPath -SummaryJsonPath $supplemental.SummaryJsonPath -ReportMdPath $supplemental.ReportMdPath
  $runSummary.ArtifactStatus.RunIndex = if ($runIndexPath) { 'OK' } else { 'Warn' }
  $artifactStatuses = @(
    $runSummary.ArtifactStatus.Csv,
    $runSummary.ArtifactStatus.Json,
    $runSummary.ArtifactStatus.SummaryJson,
    $runSummary.ArtifactStatus.ReportMd,
    $runSummary.ArtifactStatus.RunIndex
  )
  $runSummary.ArtifactStatus.Complete = -not ($artifactStatuses -contains 'Warn')
  $runSummary.Supplemental.SummaryJsonPath = $supplemental.SummaryJsonPath
  $runSummary.Supplemental.ReportMdPath = $supplemental.ReportMdPath
  $runSummary.Supplemental.RunIndexPath = $runIndexPath
  return [pscustomobject]@{
    FailedCount      = $failedCount
    ParseErrorCount  = $parseErrorCount
    RunSummary       = $runSummary
    SummaryJsonPath  = $supplemental.SummaryJsonPath
    ReportMdPath     = $supplemental.ReportMdPath
    RunIndexPath     = $runIndexPath
  }
}
