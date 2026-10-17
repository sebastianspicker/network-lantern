function Invoke-PingRaw {
  <#
  .SYNOPSIS
    Execute a native ping command and capture raw output.
  .PARAMETER Protocol
    IPv4 or IPv6.
  .PARAMETER HostName
    Target host.
  .PARAMETER Count
    Number of echo requests.
  .PARAMETER ArgBuilder4
    Scriptblock that builds the ping argument list for IPv4.
  .PARAMETER ArgBuilder6
    Scriptblock that builds the ping argument list for IPv6.
  .PARAMETER TimeoutSec
    Maximum run time in seconds before the process is killed.
  .OUTPUTS
    [pscustomobject] with Raw (string[]), ExitCode (int), TimedOut (bool) and TimeoutSec properties.
  #>
  param(
    [Parameter(Mandatory)][ValidateSet('IPv4', 'IPv6')]$Protocol,
    [Parameter(Mandatory)][string]$HostName,
    [Parameter(Mandatory)][int]$Count,
    [Parameter(Mandatory)][scriptblock]$ArgBuilder4,
    [Parameter(Mandatory)][scriptblock]$ArgBuilder6,
    [Parameter(Mandatory)][int]$TimeoutSec
  )

  $toolArgs = if ($Protocol -eq 'IPv6') { & $ArgBuilder6 $HostName $Count } else { & $ArgBuilder4 $HostName $Count }
  if ($toolArgs -isnot [System.Array]) { $toolArgs = @($toolArgs) }
  return Invoke-NativeToolWithTimeout -FilePath 'ping' -ArgumentList ([string[]]$toolArgs) -TimeoutSec $TimeoutSec
}
