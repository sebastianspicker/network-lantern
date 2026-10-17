function Test-TraceReachedDestination {
  <#
  .SYNOPSIS
    Decide from parsed tracert/pathping output whether the final hop is the destination.
  .DESCRIPTION
    Native exit codes are 0 even when the destination is unreachable, so the status
    is derived from the output: the last hop of the trace section must be a reply
    from the target address. Locale-independent (no text such as "Trace complete").
  .PARAMETER Raw
    Raw output lines.
  .PARAMETER Destination
    Host name or address that was traced.
  .OUTPUTS
    [pscustomobject] with Ok (bool) and Reason (string, empty when Ok).
  #>
  param(
    [AllowNull()][object]$Raw,
    [Parameter(Mandatory)][string]$Destination
  )

  $lines = @(@($Raw) | ForEach-Object { if ($null -ne $_) { "$_" } })
  $target = $Destination
  foreach ($line in $lines) {
    if ($line -match '^\s*[^\s\d].*?\[([0-9A-Fa-f:.%]+)\]') {
      $target = $Matches[1]
      break
    }
  }

  $lastHop = $null
  $expected = 1
  foreach ($line in $lines) {
    if ($line -match '^\s*(\d+)\s+(\S.*)$') {
      if ([int]$Matches[1] -ne $expected) {
        if ($null -ne $lastHop) { break }
        continue
      }
      $lastHop = $Matches[2].Trim()
      $expected++
    }
  }

  if ($null -eq $lastHop) {
    return [pscustomobject]@{ Ok = $false; Reason = 'no hops in output (target unresolved or tool failed)' }
  }
  $lastToken = ($lastHop -split '\s+')[-1].Trim('[', ']')
  if ($lastToken -ieq $target -or $lastToken -ieq $Destination) {
    return [pscustomobject]@{ Ok = $true; Reason = '' }
  }
  return [pscustomobject]@{ Ok = $false; Reason = "destination $target not reached (last hop: $lastHop)" }
}
