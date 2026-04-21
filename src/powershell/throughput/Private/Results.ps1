# CSV row and result list helpers (private to Iperf3TestSuite)

function Protect-CsvValue {
  <#
  .SYNOPSIS
  Prevents CSV injection by prefixing dangerous leading characters with a single quote.
  .DESCRIPTION
  Values starting with =, +, -, or @ can be interpreted as formulas by spreadsheet
  applications. This function prefixes such values with a single quote to neutralize them.
  #>
  [CmdletBinding()]
  [OutputType([string])]
  param(
    [AllowNull()]
    [AllowEmptyString()]
    [string]$Value
  )
  if ([string]::IsNullOrEmpty($Value)) { return $Value }
  if ($Value[0] -eq '=' -or $Value[0] -eq '+' -or $Value[0] -eq '-' -or $Value[0] -eq '@') {
    return "'$Value"
  }
  return $Value
}

function ConvertTo-Iperf3CsvRow {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [int]$No,
    [string]$Proto,
    [string]$Dir,
    [string]$DSCP,
    [int]$Streams,
    [string]$Win,
    [nullable[double]]$ThrTxMbps,
    [nullable[int]]$RetrTx,
    [nullable[double]]$ThrRxMbps,
    [nullable[double]]$LossTxPct,
    [nullable[double]]$JitterMs,
    [string]$Role,
    [nullable[int]]$DurationMs
  )
  return [pscustomobject][ordered]@{
    No          = $No
    Proto       = Protect-CsvValue $Proto
    Dir         = Protect-CsvValue $Dir
    DSCP        = Protect-CsvValue $DSCP
    Streams     = $Streams
    Win         = Protect-CsvValue $Win
    Thr_TX_Mbps = $ThrTxMbps
    Retr_TX     = $RetrTx
    Thr_RX_Mbps = $ThrRxMbps
    Loss_TX_Pct = $LossTxPct
    Jitter_ms   = $JitterMs
    Duration_ms = $DurationMs
    Role        = Protect-CsvValue $Role
  }
}

function Add-Iperf3TestResult {
  [CmdletBinding()]
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
    [object]$Run,
    [Parameter(Mandatory)]
    [pscustomobject]$Metrics
  )
  $durationMs = if ($Run.PSObject.Properties.Name -contains 'DurationMs') { $Run.DurationMs } else { $null }
  [void]$AllResultsList.Add([pscustomobject]@{
      No             = $No
      Proto          = $Proto
      Dir            = $Dir
      DSCP           = $DSCP
      Tos            = $Tos
      Streams        = $Streams
      Window         = $Window
      UdpBw          = $UdpBw
      Stack          = $Stack
      Target         = $Target
      Port           = $Port
      ExitCode       = $Run.ExitCode
      DurationMs     = $durationMs
      Metrics        = $Metrics
      Args           = $Run.Args
      RawText        = $Run.RawText
      JsonParseError = if ($Run.PSObject.Properties.Name -contains 'JsonParseError') { $Run.JsonParseError } else { $null }
    })
  [void]$CsvRowsList.Add(
    (ConvertTo-Iperf3CsvRow -No $No -Proto $Proto -Dir $Dir -DSCP $DSCP -Streams $Streams -Win $Window `
      -ThrTxMbps $Metrics.TxMbps -RetrTx $Metrics.Retr -ThrRxMbps $Metrics.RxMbps -LossTxPct $Metrics.LossPct -JitterMs $Metrics.JitterMs -DurationMs $durationMs -Role 'end')
  )
}
