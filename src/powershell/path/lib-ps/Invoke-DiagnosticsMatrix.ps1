function Invoke-DiagnosticsMatrix {
  <#
  .SYNOPSIS
    Iterate over every plan item, run diagnostics, collect results, and log progress.
  .PARAMETER Plan
    The full diagnostic plan (list of plan items).
  .PARAMETER PortTargets
    Array of port probe definitions.
  .PARAMETER Results
    Mutable list that receives each completed diagnostic result object.
  #>
  param(
    [Parameter(Mandatory)][System.Collections.Generic.List[object]]$Plan,
    [Parameter(Mandatory)][array]$PortTargets,
    [Parameter(Mandatory)][AllowEmptyCollection()][System.Collections.Generic.List[object]]$Results
  )

  $index = 0
  $total = $Plan.Count

  foreach ($item in $Plan) {
    $index++
    Write-Status -Level INFO -Message "RUN [$index/$total] round=$($item.RoundName) protocol=$($item.Protocol) host=$($item.Host)"

    $obj = Invoke-HostDiagnostics -PlanItem $item -PortTargets $PortTargets
    $Results.Add($obj)

    if ($obj.Tcp443OK) {
      Write-Status -Level OK -Message "round=$($item.RoundName) protocol=$($item.Protocol) host=$($item.Host) tcp443=OK"
    } else {
      Write-Status -Level FAIL -Message "round=$($item.RoundName) protocol=$($item.Protocol) host=$($item.Host) tcp443=FAIL"
    }
  }
}
