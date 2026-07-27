# Error classification helpers (private to NetworkLantern.Throughput)
# Dispatch order matters: check structured ErrorIds first (already classified), then
# regex-match exception messages from most-specific to least-specific category.
# InputValidation > Prerequisite > Connectivity > Internal (fallback)

function Resolve-Iperf3ClassifiedError {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]
    [System.Management.Automation.ErrorRecord]$ErrorRecord
  )
  $msg = [string]$ErrorRecord.Exception.Message
  $fqid = [string]$ErrorRecord.FullyQualifiedErrorId

  if ($fqid -like 'NetworkLantern.Throughput.*') {
    return [pscustomobject]@{
      ErrorId  = ($fqid -split ',')[0]
      Category = [System.Management.Automation.ErrorCategory]::NotSpecified
      Message  = $msg
    }
  }

  if (
    $ErrorRecord.Exception -is [System.Management.Automation.ParameterBindingException] -or
    $fqid -match 'ParameterArgumentValidationError|ParameterBinding' -or
    $msg -match 'Cannot validate argument on parameter|Cannot bind parameter|Invalid value for key|Target is required|Invalid Target|ProfileName is required|Profile .+ not found|At least one DSCP class is required|Profiles file path must be under|Configuration path must be under|invalid characters|\.json extension|exceeds maximum length|control characters|must not look like an option|Unknown parameter key|Invalid TCP window size|Invalid DSCP class|Unknown DSCP class'
  ) {
    return [pscustomobject]@{
      ErrorId  = 'NetworkLantern.Throughput.InputValidation'
      Category = [System.Management.Automation.ErrorCategory]::InvalidArgument
      Message  = $msg
    }
  }

  if ($msg -match 'iperf3.*required|only supported on Windows|ping.exe is required|profiles file is invalid|Profiles file exceeds maximum size') {
    return [pscustomobject]@{
      ErrorId  = 'NetworkLantern.Throughput.Prerequisite'
      Category = [System.Management.Automation.ErrorCategory]::ResourceUnavailable
      Message  = $msg
    }
  }

  if ($msg -match 'ICMP reachability|TCP port') {
    return [pscustomobject]@{
      ErrorId  = 'NetworkLantern.Throughput.Connectivity'
      Category = [System.Management.Automation.ErrorCategory]::ConnectionError
      Message  = $msg
    }
  }

  Write-Verbose "Error classified as Internal (unmatched pattern): $msg"
  return [pscustomobject]@{
    ErrorId  = 'NetworkLantern.Throughput.Internal'
    Category = [System.Management.Automation.ErrorCategory]::NotSpecified
    Message  = $msg
  }
}

function New-Iperf3ClassifiedErrorRecord {
  [CmdletBinding()]
  [OutputType([System.Management.Automation.ErrorRecord])]
  param(
    [Parameter(Mandatory)]
    [System.Management.Automation.ErrorRecord]$ErrorRecord
  )
  $meta = Resolve-Iperf3ClassifiedError -ErrorRecord $ErrorRecord
  $inner = $ErrorRecord.Exception
  $ex = New-Object System.Exception($meta.Message, $inner)
  $classified = New-Object System.Management.Automation.ErrorRecord($ex, $meta.ErrorId, $meta.Category, $null)
  return $classified
}
