function Invoke-NativeToolWithTimeout {
  <#
  .SYNOPSIS
    Run a native tool with a hard timeout and capture its output lines.
  .PARAMETER FilePath
    Tool name or path (for example ping, tracert, pathping).
  .PARAMETER ArgumentList
    Arguments passed to the tool without shell interpretation.
  .PARAMETER TimeoutSec
    Maximum run time in seconds. The process tree is killed when it is exceeded.
  .OUTPUTS
    [pscustomobject] with Raw (string[]), ExitCode (int, $null on timeout), TimedOut (bool) and TimeoutSec.
  #>
  param(
    [Parameter(Mandatory)][string]$FilePath,
    [Parameter(Mandatory)][string[]]$ArgumentList,
    [Parameter(Mandatory)][ValidateRange(1, 86400)][int]$TimeoutSec
  )

  $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
  $startInfo.FileName = $FilePath
  foreach ($arg in $ArgumentList) { $startInfo.ArgumentList.Add($arg) }
  $startInfo.UseShellExecute = $false
  $startInfo.CreateNoWindow = $true
  $startInfo.RedirectStandardOutput = $true
  $startInfo.RedirectStandardError = $true
  try {
    $startInfo.StandardOutputEncoding = [System.Console]::OutputEncoding
    $startInfo.StandardErrorEncoding = [System.Console]::OutputEncoding
  } catch {
    Write-Verbose "Keeping default process output encoding: $($_.Exception.Message)"
  }

  $process = [System.Diagnostics.Process]::new()
  $process.StartInfo = $startInfo
  try {
    $null = $process.Start()
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $exited = $process.WaitForExit($TimeoutSec * 1000)
    if (-not $exited) {
      try { $process.Kill($true) } catch { Write-Verbose "Kill failed: $($_.Exception.Message)" }
      $null = $process.WaitForExit(5000)
    } else {
      $process.WaitForExit()
    }
    $text = ''
    if ($stdoutTask.Wait(5000)) { $text += $stdoutTask.Result }
    if ($stderrTask.Wait(1000)) { $text += $stderrTask.Result }
    $lines = if ($text.Length -gt 0) { @($text -split "\r?\n") } else { @() }
    return [pscustomobject]@{
      Raw        = $lines
      ExitCode   = if ($exited) { $process.ExitCode } else { $null }
      TimedOut   = (-not $exited)
      TimeoutSec = $TimeoutSec
    }
  } finally {
    $process.Dispose()
  }
}

function Get-NativeToolTimeoutSec {
  <#
  .SYNOPSIS
    Return the per-tool timeout: the explicit override, or an estimate from probes x timeout x hops plus margin.
  .DESCRIPTION
    The result is clamped to the 1..86400 range accepted by Invoke-NativeToolWithTimeout.
    Pathping first traces the route (up to hops x per-hop timeout), then spends about
    probes x 0.25 s (its default /p period) on every hop while computing statistics.
  #>
  param(
    [Parameter(Mandatory)][ValidateSet('Ping', 'Tracert', 'Pathping')][string]$Tool,
    [Parameter(Mandatory)][object]$Settings,
    [Parameter(Mandatory)][object]$Round
  )

  $maxTimeoutSec = 86400
  $margin = 30
  [double]$seconds = if ($Settings.ToolTimeoutSec -gt 0) {
    [double]$Settings.ToolTimeoutSec
  } else {
    switch ($Tool) {
      'Ping' { [double]$Settings.PingCount * 5 + $margin }
      'Tracert' { [double]$Round.TraceHops * 3 * [math]::Ceiling($Round.TraceWaitMs / 1000) + $margin }
      'Pathping' {
        # pathping is not given /h, so it traces up to its default of 30 hops.
        $hops = [math]::Max([double]$Round.TraceHops, 30)
        $perHopWaitSec = [math]::Ceiling($Round.PathpingWaitMs / 1000)
        $pingPeriodSec = 0.25
        $hops * $perHopWaitSec + $hops * [double]$Settings.PathpingProbes * $pingPeriodSec + $margin
      }
    }
  }
  return [int][math]::Min($maxTimeoutSec, [math]::Max(1, [math]::Ceiling($seconds)))
}
