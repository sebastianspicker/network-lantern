Set-StrictMode -Version Latest

# Default parameter set for Invoke-Iperf3TestSuite (single source for CLI merge)
$script:DefaultInvokeIperf3TestSuiteParams = @{
  Target                = $null
  Port                  = 5201
  Duration              = 10
  Omit                  = 1
  OutDir                = $null
  Quiet                 = $false
  Progress              = $false
  Summary               = $false
  DisableMtuProbe       = $false
  SkipReachabilityCheck = $false
  Force                 = $false
  WhatIf                = $false
  Protocol              = 'Both'
  SingleTest            = $false
  MtuSizes              = @(1400, 1472, 1600)
  ConnectTimeoutMs      = 60000
  UdpStart              = '1M'
  UdpMax                = '1G'
  UdpStep               = '10M'
  UdpLossThreshold      = 5.0
  TcpStreams            = @(1, 4, 8)
  TcpWindows            = @('default', '128K', '256K')
  DscpClasses           = @('CS0', 'AF11', 'CS5', 'EF', 'AF41')
  IpVersion             = 'Auto'
  ProfileName           = $null
  ProfilesFile          = $null
  SaveProfile           = $false
  ListProfiles          = $false
  StrictConfiguration   = $false
  PassThru              = $false
  RetryCount            = 0
  ThresholdMinThroughputMbps = $null
  ThresholdMaxLossPct        = $null
  ThresholdMaxJitterMs       = $null
}

# Load private helpers (order: no cross-file deps first, then dependents)
$privateDir = Join-Path $PSScriptRoot 'Private'
$privateScripts = @(
  'Common.ps1'
  'Validation.ps1'
  'Conversion.ps1'
  'JsonParsing.ps1'
  'ConfigValidation.ps1'
  'ErrorClassification.ps1'
  'Connectivity.ps1'
  'Results.ps1'
  'Profiles.ps1'
  'Reporting.ps1'
  'Iperf3Run.ps1'
  'Orchestration.ps1'
)
$missing = @($privateScripts | Where-Object { -not (Test-Path -LiteralPath (Join-Path $privateDir $_)) })
if ($missing.Count -gt 0) {
  throw "Iperf3TestSuite: Missing private script(s): $($missing -join ', '). Path: $privateDir"
}
foreach ($name in $privateScripts) {
  . (Join-Path $privateDir $name)
}

function Get-Iperf3TestSuiteDefaultParameterSet {
  <#
  .SYNOPSIS
  Returns the default parameter set for Invoke-Iperf3TestSuite.
  .OUTPUTS
  [hashtable]
  #>
  [CmdletBinding()]
  [OutputType([hashtable])]
  param()
  $src = $script:DefaultInvokeIperf3TestSuiteParams
  $h = @{}
  foreach ($key in $src.Keys) {
    $val = $src[$key]
    if ($null -eq $val) { $h[$key] = $null }
    elseif ($val -is [array]) { $h[$key] = @($val) }
    else { $h[$key] = $val }
  }
  if (-not $h['OutDir']) { $h['OutDir'] = Join-Path (Get-Location) 'logs' }
  if (-not $h['ProfilesFile']) { $h['ProfilesFile'] = Get-DefaultProfilesFilePath }
  return $h
}

function Invoke-Iperf3TestSuite {
  <#
  .SYNOPSIS
  Runs a TCP/UDP iperf3 test matrix and writes CSV/JSON artifacts.
  .DESCRIPTION
  Executes a DSCP-marked TCP and UDP test suite against an iperf3 server.
  Full features on Windows (ping.exe, Test-NetConnection). Linux/macOS supported with
  -SkipReachabilityCheck -DisableMtuProbe (TCP port check uses TcpClient fallback).

  The function produces CSV/JSON results, a summary JSON, and a Markdown report in OutDir.
  Use -PassThru to receive the run summary object on the pipeline.
  Use -WhatIf to preview the test plan without running any tests.
  .PARAMETER Target
  Hostname or IP address of the iperf3 server.
  .PARAMETER Protocol
  Which protocols to test: TCP, UDP, or Both (default: Both).
  .PARAMETER SingleTest
  Run only one test (TCP TX or UDP TX for the first DSCP class) for quick validation.
  .PARAMETER PassThru
  Return the run summary object instead of writing only to files.
  .PARAMETER WhatIf
  Preview mode: calculate and display the test plan without executing any tests.
  .EXAMPLE
  Invoke-Iperf3TestSuite -Target 'iperf3.example.com' -Port 5201
  .EXAMPLE
  Invoke-Iperf3TestSuite -Target '10.0.0.1' -SingleTest -PassThru
  .EXAMPLE
  $summary = Invoke-Iperf3TestSuite -Target '10.0.0.1' -Protocol TCP -DscpClasses @('CS0','EF') -PassThru
  #>
  [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSupportsShouldProcess', '', Justification = 'Custom -WhatIf switch is intentionally passed through for preview behavior without state mutation.')]
  [CmdletBinding()]
  param(
    [string]$Target,
    [ValidateRange(1, 65535)]
    [int]$Port = 5201,
    [ValidateRange(1, 3600)]
    [int]$Duration = 10,
    [ValidateRange(0, 60)]
    [int]$Omit = 1,
    [ValidateNotNullOrEmpty()]
    [string]$OutDir = (Join-Path (Get-Location) 'logs'),
    [switch]$Quiet,
    [switch]$Progress,
    [switch]$Summary,
    [switch]$DisableMtuProbe,
    [switch]$SkipReachabilityCheck,
    [ValidateNotNullOrEmpty()]
    [int[]]$MtuSizes = @(1400, 1472, 1600),
    [ValidateRange(1000, 300000)]
    [int]$ConnectTimeoutMs = 60000,
    [ValidateNotNullOrEmpty()]
    [string]$UdpStart = '1M',
    [ValidateNotNullOrEmpty()]
    [string]$UdpMax = '1G',
    [ValidateNotNullOrEmpty()]
    [string]$UdpStep = '10M',
    [ValidateRange(0, 100)]
    [double]$UdpLossThreshold = 5.0,
    [ValidateNotNullOrEmpty()]
    [int[]]$TcpStreams = @(1, 4, 8),
    [ValidateNotNullOrEmpty()]
    [string[]]$TcpWindows = @('default', '128K', '256K'),
    [ValidateNotNullOrEmpty()]
    [string[]]$DscpClasses = @('CS0', 'AF11', 'CS5', 'EF', 'AF41'),
    [ValidateSet('IPv4', 'IPv6', 'Auto')]
    [string]$IpVersion = 'Auto',
    [ValidateSet('TCP', 'UDP', 'Both')]
    [string]$Protocol = 'Both',
    [switch]$SingleTest,
    [ValidateRange(0, 5)]
    [int]$RetryCount = 0,
    [switch]$Force,
    [switch]$WhatIf,
    [string]$ProfileName,
    [string]$ProfilesFile,
    [switch]$SaveProfile,
    [switch]$ListProfiles,
    [switch]$StrictConfiguration,
    [switch]$PassThru,
    [ValidateRange(0, [double]::MaxValue)]
    [nullable[double]]$ThresholdMinThroughputMbps,
    [ValidateRange(0, 100)]
    [nullable[double]]$ThresholdMaxLossPct,
    [ValidateRange(0, [double]::MaxValue)]
    [nullable[double]]$ThresholdMaxJitterMs
  )

  # Save and override ErrorActionPreference to ensure all errors are terminating within
  # the test suite. This is the accepted PowerShell pattern: save before try, restore in
  # finally. Risk: if a called function relies on the caller's EAP being something other
  # than 'Stop', it may behave differently. The finally block guarantees restoration even
  # if an exception is thrown.
  $oldEap = $ErrorActionPreference
  $ErrorActionPreference = 'Stop'

  try {
    $effective = Get-Iperf3TestSuiteDefaultParameterSet
    foreach ($k in $PSBoundParameters.Keys) {
      if ($effective.ContainsKey($k)) { $effective[$k] = $PSBoundParameters[$k] }
    }

    $strict = [bool]$effective['StrictConfiguration']
    $allowedKeys = @($script:DefaultInvokeIperf3TestSuiteParams.Keys)

    if (-not $effective['ProfilesFile']) { $effective['ProfilesFile'] = Get-DefaultProfilesFilePath }
    $effective['ProfilesFile'] = Resolve-ProfilesFilePath -ProfilesFile $effective['ProfilesFile']

    if ($effective['ProfileName'] -and -not $effective['SaveProfile'] -and -not $effective['ListProfiles']) {
      $profileParams = Get-Iperf3ProfileParameters -ProfileName $effective['ProfileName'] -ProfilesFile $effective['ProfilesFile'] -StrictConfiguration:$strict
      $profileValidation = ConvertTo-Iperf3NormalizedParameterSet -InputParameters $profileParams -AllowedKeys $allowedKeys -StrictConfiguration:$strict
      foreach ($w in $profileValidation.Warnings) { Write-Warning $w }
      foreach ($k in $profileValidation.Parameters.Keys) { $effective[$k] = $profileValidation.Parameters[$k] }
      foreach ($k in $PSBoundParameters.Keys) {
        if ($effective.ContainsKey($k)) { $effective[$k] = $PSBoundParameters[$k] }
      }
    }

    if ($effective['ListProfiles']) {
      $names = Get-Iperf3ProfileNames -ProfilesFile $effective['ProfilesFile'] -StrictConfiguration:$strict
      if (-not $effective['Quiet']) {
        Write-Information -InformationAction Continue "Profiles file: $($effective['ProfilesFile'])"
        if ($names.Count -eq 0) {
          Write-Information -InformationAction Continue 'No profiles found.'
        }
        else {
          foreach ($n in $names) { Write-Information -InformationAction Continue "- $n" }
        }
      }
      if ($effective['PassThru']) {
        return [pscustomobject]@{
          Mode        = 'ListProfiles'
          Profiles    = $names
          ProfilesFile = $effective['ProfilesFile']
        }
      }
      return
    }

    if (-not $effective['Target']) {
      throw "Target is required. Provide -Target 'hostname' directly or via a saved profile (-ProfileName)."
    }
    if (-not (Test-ValidHostnameOrIP -Name ([string]$effective['Target']))) {
      throw "Invalid Target: '$($effective['Target'])'. Must be a valid hostname or IP address."
    }

    if ($effective['SaveProfile']) {
      if (-not $effective['ProfileName']) {
        throw 'ProfileName is required when using -SaveProfile.'
      }
      $saveResult = Save-Iperf3Profile -ProfileName $effective['ProfileName'] -ProfilesFile $effective['ProfilesFile'] -Parameters $effective -StrictConfiguration:$strict
      if (-not $effective['Quiet']) {
        Write-Information -InformationAction Continue "Saved profile '$($saveResult.ProfileName)' to '$($saveResult.ProfilesFile)'."
      }
    }

    $startedUtc = (Get-Date).ToUniversalTime()
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $ts = Get-Date -Format 'yyyyMMdd_HHmmss_fff'
    $jsonPath = Join-Path -Path $effective['OutDir'] -ChildPath "iperf3_results_$ts.json"
    $csvPath = Join-Path -Path $effective['OutDir'] -ChildPath "iperf3_summary_$ts.csv"

    $caps = $null
    if ([bool]$effective['WhatIf']) {
      try {
        $caps = Get-Iperf3Capability
      }
      catch {
        $caps = [pscustomobject]@{
          VersionText    = 'unknown (iperf3 not available)'
          Major          = $null
          Minor          = $null
          BidirSupported = $false
        }
      }
    }
    else {
      Test-Iperf3TestSuitePrerequisites -SkipReachabilityCheck:([bool]$effective['SkipReachabilityCheck']) -DisableMtuProbe:([bool]$effective['DisableMtuProbe'])
      $caps = Get-Iperf3Capability
      if ($null -ne $caps.Major -and $null -ne $caps.Minor) {
        if ($caps.Major -lt 3 -or ($caps.Major -eq 3 -and $caps.Minor -lt 7)) {
          throw "iperf3 3.7 or newer is required (detected: $($caps.VersionText))."
        }
      }
    }

    if ([bool]$effective['SingleTest'] -and @($effective['DscpClasses']).Count -eq 0) {
      throw "At least one DSCP class is required. When using -SingleTest, ensure DscpClasses contains at least one value (e.g. 'CS0')."
    }

    $plan = Build-TestPlan -SingleTest:([bool]$effective['SingleTest']) -Protocol ([string]$effective['Protocol']) -DscpClasses @($effective['DscpClasses']) -TcpStreams @($effective['TcpStreams']) -TcpWindows @($effective['TcpWindows']) -Caps $caps -UdpStart ([string]$effective['UdpStart']) -UdpMax ([string]$effective['UdpMax']) -UdpStep ([string]$effective['UdpStep'])

    if ([bool]$effective['WhatIf']) {
      if (-not $effective['Quiet']) {
        Write-Information -InformationAction Continue "WhatIf: Would run approximately $($plan.TotalApprox) tests. Target: $($effective['Target']) Port: $($effective['Port']) Protocol: $($effective['Protocol'])."
        Write-Information -InformationAction Continue "CSV  : $csvPath"
        Write-Information -InformationAction Continue "JSON : $jsonPath"
      }
      if ($effective['PassThru']) {
        return [pscustomobject]@{
          Mode               = 'WhatIf'
          TotalApprox        = $plan.TotalApprox
          CsvPath            = $csvPath
          JsonPath           = $jsonPath
          EffectiveParameters = $effective
        }
      }
      return
    }

    $null = New-Item -ItemType Directory -Path $effective['OutDir'] -Force

    $conn = Get-TestSuiteConnectivity -Target ([string]$effective['Target']) -Port ([int]$effective['Port']) -IpVersion ([string]$effective['IpVersion']) -SkipReachabilityCheck:([bool]$effective['SkipReachabilityCheck']) -DisableMtuProbe:([bool]$effective['DisableMtuProbe']) -MtuSizes @($effective['MtuSizes']) -ConnectTimeoutMs ([int]$effective['ConnectTimeoutMs'])
    $stack = $conn.Stack
    $net = $conn.Net
    $mtuFails = $conn.MtuFails

    if (-not $effective['Quiet']) {
      Write-Information -InformationAction Continue "Target: $($effective['Target']) Port: $($effective['Port']) Stack: $stack (~$($plan.TotalApprox) tests)"
      Write-Information -InformationAction Continue "CSV  : $csvPath"
      Write-Information -InformationAction Continue "JSON : $jsonPath"
    }

    if ((Test-Path -LiteralPath $csvPath) -or (Test-Path -LiteralPath $jsonPath)) {
      if (-not $effective['Force']) { throw 'Output file(s) already exist. Use -Force to overwrite.' }
    }

    $allResults = New-Object System.Collections.Generic.List[object]
    $csvRows = New-Object System.Collections.Generic.List[object]
    $testNo = 0

    # Make retry count available to Invoke-SingleIperf3TestAndAddResult throughout the run.
    $script:ActiveRetryCount = [int]$effective['RetryCount']

    # Shared parameters splatted to all matrix functions to reduce per-call boilerplate.
    $sharedParams = @{
      AllResultsList = $allResults
      CsvRowsList    = $csvRows
      Stack          = $stack
      Target         = [string]$effective['Target']
      Port           = [int]$effective['Port']
      Duration       = [int]$effective['Duration']
      Omit           = [int]$effective['Omit']
      ConnectTimeoutMs = [int]$effective['ConnectTimeoutMs']
      Caps           = $caps
      Progress       = [bool]$effective['Progress']
      TotalApprox    = $plan.TotalApprox
    }

    foreach ($dscp in $plan.DscpList) {
      $tos = Get-TosFromDscpClass -Class $dscp
      if ($plan.RunTcp) {
        Invoke-TcpMatrix @sharedParams -TestNoRef ([ref]$testNo) -Dscp $dscp -Tos $tos -DirsTcpList $plan.DirsTcpList -TcpStreamsList $plan.TcpStreamsList -TcpWindowsList $plan.TcpWindowsList
      }
      if ($plan.RunUdp) {
        Invoke-UdpMatrix @sharedParams -TestNoRef ([ref]$testNo) -Dscp $dscp -Tos $tos -UdpStart ([string]$effective['UdpStart'])
      }
      if ($plan.RunSingleUdp) {
        Invoke-UdpSingleTest @sharedParams -TestNoRef ([ref]$testNo) -Dscp $dscp -Tos $tos -UdpStart ([string]$effective['UdpStart'])
      }
      if ($plan.RunUdp) {
        Invoke-UdpSaturationMatrix @sharedParams -TestNoRef ([ref]$testNo) -Dscp $dscp -Tos $tos -UdpLossThreshold ([double]$effective['UdpLossThreshold']) -CurMbps $plan.CurMbps -MaxMbps $plan.MaxMbps -StepMbps $plan.StepMbps
      }
    }

    $final = [pscustomobject]@{
      Timestamp      = $ts
      Target         = [string]$effective['Target']
      Port           = [int]$effective['Port']
      Stack          = $stack
      Iperf3Version  = $caps.VersionText
      BidirSupported = $caps.BidirSupported
      MtuProbe       = [pscustomobject]@{
        Enabled     = (-not [bool]$effective['DisableMtuProbe'])
        Sizes       = @($effective['MtuSizes'])
        FailedSizes = @($mtuFails)
      }
      NetConnection  = [pscustomobject]@{
        TcpTestSucceeded = $net.Tcp.TcpTestSucceeded
        RemoteAddress    = $net.Tcp.RemoteAddress
        PingSucceeded    = $net.Tcp.PingSucceeded
        TraceRoute       = if ($net.Trace) { $net.Trace.TraceRoute } else { $null }
      }
      Results        = $allResults.ToArray()
    }

    $stopwatch.Stop()
    $completedUtc = (Get-Date).ToUniversalTime()

    $finalize = Write-FinalOutputs -CsvRowsList $csvRows -AllResultsList $allResults -CsvPath $csvPath -JsonPath $jsonPath -FinalResultObject $final -OutDir ([string]$effective['OutDir']) -Timestamp $ts -StartedUtc $startedUtc.ToString('o') -CompletedUtc $completedUtc.ToString('o') -ElapsedSeconds ([math]::Round($stopwatch.Elapsed.TotalSeconds, 1)) -Iperf3Version $caps.VersionText -ThresholdMinThroughputMbps $effective['ThresholdMinThroughputMbps'] -ThresholdMaxLossPct $effective['ThresholdMaxLossPct'] -ThresholdMaxJitterMs $effective['ThresholdMaxJitterMs']
    $final | Add-Member -MemberType NoteProperty -Name Supplemental -Value ([pscustomobject]@{
        SummaryJsonPath = $finalize.SummaryJsonPath
        ReportMdPath    = $finalize.ReportMdPath
        RunIndexPath    = $finalize.RunIndexPath
      }) -Force

    if (-not $effective['Quiet']) {
      Write-Information -InformationAction Continue "CSV  : $csvPath"
      Write-Information -InformationAction Continue "JSON : $jsonPath"
      Write-Information -InformationAction Continue "Summary JSON: $($finalize.SummaryJsonPath)"
      Write-Information -InformationAction Continue "Report MD  : $($finalize.ReportMdPath)"
      Write-Information -InformationAction Continue "Run index  : $($finalize.RunIndexPath)"
      Write-Information -InformationAction Continue "Completed $testNo tests; $($finalize.FailedCount) failed. Elapsed: $([math]::Round($stopwatch.Elapsed.TotalSeconds, 1))s"
      if ($finalize.ParseErrorCount -gt 0) { Write-Information -InformationAction Continue "$($finalize.ParseErrorCount) test(s) with JSON parse errors." }
      if ($testNo -eq 0 -or $finalize.FailedCount -eq $testNo) { Write-Warning 'No tests completed successfully.' }
      if ($effective['Summary']) { Write-Information -InformationAction Continue "Tests: $testNo total" }
    }

    if ($effective['PassThru']) {
      return $finalize.RunSummary
    }
  }
  catch {
    throw (New-Iperf3ClassifiedErrorRecord -ErrorRecord $_)
  }
  finally {
    $script:ActiveRetryCount = $null
    $ErrorActionPreference = $oldEap
  }
}

function Compare-Iperf3Runs {
  <#
  .SYNOPSIS
  Compares two iperf3 run summary JSON files and outputs a delta object.
  .DESCRIPTION
  Reads two summary JSON files (produced by Invoke-Iperf3TestSuite) and computes
  differences in status, counts, and elapsed time. Useful for CI regression
  detection when comparing a baseline summary against the current summary.
  .PARAMETER BaselinePath
  Path to the baseline (previous) summary JSON file.
  .PARAMETER CurrentPath
  Path to the current summary JSON file.
  .OUTPUTS
  [pscustomobject] Delta object with baseline/current status, counts, and timing changes.
  .EXAMPLE
  $delta = Compare-Iperf3Runs -BaselinePath ./prev_summary.json -CurrentPath ./curr_summary.json
  if ($delta.StatusChanged) { Write-Warning "Status changed: $($delta.BaselineStatus) -> $($delta.CurrentStatus)" }
  #>
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]
    [string]$BaselinePath,
    [Parameter(Mandatory)]
    [string]$CurrentPath
  )
  if (-not (Test-Path -LiteralPath $BaselinePath -PathType Leaf)) {
    throw "Baseline summary file not found: $BaselinePath"
  }
  if (-not (Test-Path -LiteralPath $CurrentPath -PathType Leaf)) {
    throw "Current summary file not found: $CurrentPath"
  }
  $baseline = Get-Content -LiteralPath $BaselinePath -Raw -Encoding UTF8 | ConvertFrom-Json
  $current = Get-Content -LiteralPath $CurrentPath -Raw -Encoding UTF8 | ConvertFrom-Json

  $requiredProps = @('Status', 'Counts', 'Timestamp')
  foreach ($prop in $requiredProps) {
    if ($baseline.PSObject.Properties.Name -notcontains $prop) {
      throw "Baseline summary file is missing required property '$prop': $BaselinePath"
    }
    if ($current.PSObject.Properties.Name -notcontains $prop) {
      throw "Current summary file is missing required property '$prop': $CurrentPath"
    }
  }

  return [pscustomobject]@{
    BaselineTimestamp = $baseline.Timestamp
    CurrentTimestamp  = $current.Timestamp
    BaselineStatus    = $baseline.Status
    CurrentStatus     = $current.Status
    StatusChanged     = $baseline.Status -ne $current.Status
    BaselineCounts    = $baseline.Counts
    CurrentCounts     = $current.Counts
    FailedDelta       = ([int]$current.Counts.Failed) - ([int]$baseline.Counts.Failed)
    TotalDelta        = ([int]$current.Counts.Total) - ([int]$baseline.Counts.Total)
    BaselineElapsed   = $baseline.ElapsedSeconds
    CurrentElapsed    = $current.ElapsedSeconds
  }
}

Export-ModuleMember -Function Invoke-Iperf3TestSuite, Get-Iperf3TestSuiteDefaultParameterSet, Get-Iperf3ProfileNames, Get-Iperf3ProfileParameters, Save-Iperf3Profile, Remove-Iperf3Profile, Compare-Iperf3Runs
