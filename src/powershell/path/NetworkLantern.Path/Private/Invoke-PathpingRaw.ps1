function Invoke-PathpingRaw {
  <#
  .SYNOPSIS
    Execute a native pathping command and capture raw output.
  .PARAMETER Protocol
    IPv4 or IPv6.
  .PARAMETER HostName
    Target host.
  .PARAMETER ArgBuilder
    Scriptblock that builds the pathping argument list given (ipSwitch, host).
  .PARAMETER TimeoutSec
    Maximum run time in seconds before the process is killed.
  .OUTPUTS
    [pscustomobject] with Raw (string[]), ExitCode (int), TimedOut (bool) and TimeoutSec properties.
  #>
  param(
    [Parameter(Mandatory)][ValidateSet('IPv4', 'IPv6')]$Protocol,
    [Parameter(Mandatory)][string]$HostName,
    [Parameter(Mandatory)][scriptblock]$ArgBuilder,
    [Parameter(Mandatory)][int]$TimeoutSec
  )

  $sw = Get-ToolIpSwitch -Protocol $Protocol
  $toolArgs = & $ArgBuilder $($sw.Pathping) $HostName
  if ($toolArgs -isnot [System.Array]) { $toolArgs = @($toolArgs) }
  return Invoke-NativeToolWithTimeout -FilePath 'pathping' -ArgumentList ([string[]]$toolArgs) -TimeoutSec $TimeoutSec
}
