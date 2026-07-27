# Native process lifecycle and cancellation helpers (private to NetworkLantern.Throughput)

function Get-Iperf3DescendantProcessSnapshot {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]
    [int]$RootProcessId
  )
  $allProcesses = @()
  try {
    $allProcesses = @(Get-Process -ErrorAction Stop)
    $childrenByParent = @{}
    $processById = @{}
    $snapshotComplete = $true
    $snapshotErrors = [System.Collections.Generic.List[string]]::new()
    foreach ($candidate in $allProcesses) {
      $processById[[int]$candidate.Id] = $candidate
      $parent = $null
      try {
        $parent = $candidate.Parent
        if ($parent) {
          $parentId = [int]$parent.Id
          if (-not $childrenByParent.ContainsKey($parentId)) {
            $childrenByParent[$parentId] = [System.Collections.Generic.List[int]]::new()
          }
          $childrenByParent[$parentId].Add([int]$candidate.Id)
        }
      }
      catch {
        $parentError = $_.Exception.Message
        $candidateExited = $false
        try { $candidateExited = [bool]$candidate.HasExited }
        catch { Write-Verbose "Process $($candidate.Id) state became unavailable during the descendant snapshot." }
        if ($candidateExited) {
          Write-Verbose "Process $($candidate.Id) exited while its parent relationship was inspected."
        }
        else {
          $snapshotComplete = $false
          $snapshotErrors.Add("Parent relationship unavailable for process $($candidate.Id): $parentError")
        }
      }
      finally {
        if ($parent) { $parent.Dispose() }
      }
    }

    $descendantIds = [System.Collections.Generic.List[int]]::new()
    $pending = [System.Collections.Generic.Queue[int]]::new()
    $pending.Enqueue($RootProcessId)
    while ($pending.Count -gt 0) {
      $parentId = $pending.Dequeue()
      if (-not $childrenByParent.ContainsKey($parentId)) { continue }
      foreach ($childId in $childrenByParent[$parentId]) {
        if ($descendantIds.Contains($childId)) { continue }
        $descendantIds.Add($childId)
        $pending.Enqueue($childId)
      }
    }

    $descendants = @($descendantIds | ForEach-Object { $processById[$_] })
    foreach ($candidate in $allProcesses) {
      if ($descendantIds -notcontains [int]$candidate.Id) { $candidate.Dispose() }
    }
    return [pscustomobject]@{
      Succeeded   = $snapshotComplete
      Processes  = $descendants
      ProcessIds = [int[]]@($descendantIds)
      Error      = if ($snapshotComplete) { $null } else { $snapshotErrors -join ' ' }
    }
  }
  catch {
    foreach ($candidate in $allProcesses) {
      try { $candidate.Dispose() } catch { Write-Verbose "Could not dispose process snapshot $($candidate.Id)." }
    }
    return [pscustomobject]@{
      Succeeded   = $false
      Processes  = @()
      ProcessIds = [int[]]@()
      Error      = $_.Exception.Message
    }
  }
}

function Stop-Iperf3ProcessAfterTimeout {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]
    [object]$Process,
    [ValidateRange(1, 30000)]
    [int]$GracePeriodMs = 5000
  )
  $processId = [int]$Process.Id
  $snapshot = Get-Iperf3DescendantProcessSnapshot -RootProcessId $processId
  $trackedProcesses = @($Process) + @($snapshot.Processes)
  if ($Process.HasExited) {
    foreach ($descendant in $snapshot.Processes) { $descendant.Dispose() }
    return [pscustomobject]@{
      TerminationSucceeded     = $false
      RootExited               = $true
      TreeTerminationVerified  = $false
      TerminationScope         = 'RootOnly'
      ProcessId                = $processId
      DescendantProcessIds     = [int[]]@($snapshot.ProcessIds)
      UnterminatedProcessIds   = [int[]]@()
      Error                    = 'Root exited before termination ownership could be established; orphaned or reparented descendants cannot be ruled out.'
    }
  }
  try {
    $Process.Kill($true)
  }
  catch {
    foreach ($descendant in $snapshot.Processes) { $descendant.Dispose() }
    return [pscustomobject]@{
      TerminationSucceeded     = $false
      RootExited               = $false
      TreeTerminationVerified  = $false
      TerminationScope         = if ($snapshot.Succeeded) { 'TrackedProcessTree' } else { 'RootOnly' }
      ProcessId                = $processId
      DescendantProcessIds     = [int[]]@($snapshot.ProcessIds)
      UnterminatedProcessIds   = [int[]]@($trackedProcesses | ForEach-Object { [int]$_.Id })
      Error                    = "Kill failed: $($_.Exception.Message)"
    }
  }

  $deadline = [System.Diagnostics.Stopwatch]::StartNew()
  $unterminated = @($trackedProcesses)
  while ($unterminated.Count -gt 0 -and $deadline.ElapsedMilliseconds -lt $GracePeriodMs) {
    $unterminated = @($unterminated | Where-Object {
        try { -not $_.HasExited } catch { $true }
      })
    if ($unterminated.Count -gt 0) {
      $remainingMs = $GracePeriodMs - [int]$deadline.ElapsedMilliseconds
      Start-Sleep -Milliseconds ([math]::Min(25, [math]::Max($remainingMs, 1)))
    }
  }
  $deadline.Stop()
  $unterminated = @($trackedProcesses | Where-Object {
      try { -not $_.HasExited } catch { $true }
    })
  $rootExited = -not ($unterminated | Where-Object { [int]$_.Id -eq $processId })
  $treeVerified = [bool]$snapshot.Succeeded -and $unterminated.Count -eq 0
  $unterminatedIds = [int[]]@($unterminated | ForEach-Object { [int]$_.Id })
  foreach ($descendant in $snapshot.Processes) { $descendant.Dispose() }

  $errorText = $null
  if (-not $snapshot.Succeeded) {
    $errorText = "Root termination was attempted, but descendants could not be enumerated: $($snapshot.Error)"
  }
  elseif ($unterminatedIds.Count -gt 0) {
    $errorText = "Tracked process IDs remained alive after the ${GracePeriodMs}ms termination grace period: $($unterminatedIds -join ', ')."
  }
  return [pscustomobject]@{
    TerminationSucceeded     = ($rootExited -and $treeVerified)
    RootExited               = $rootExited
    TreeTerminationVerified  = $treeVerified
    TerminationScope         = if ($snapshot.Succeeded) { 'TrackedProcessTree' } else { 'RootOnly' }
    ProcessId                = $processId
    DescendantProcessIds     = [int[]]@($snapshot.ProcessIds)
    UnterminatedProcessIds   = $unterminatedIds
    Error                    = $errorText
  }
}

function New-Iperf3CancellationException {
  [CmdletBinding()]
  [OutputType([System.Exception])]
  param(
    [Parameter(Mandatory)]
    [pscustomobject]$NativeProcess,
    [string]$RunId
  )
  $cleanupVerified = [bool]$NativeProcess.TerminationSucceeded -and
    [bool]$NativeProcess.RootExited -and
    [bool]$NativeProcess.TreeTerminationVerified -and
    [bool]$NativeProcess.StreamsCompleted
  $message = if ($cleanupVerified) {
    "NETWORK_LANTERN_IPERF3_CANCELLED run=$RunId process=$($NativeProcess.ProcessId) tracked process tree terminated."
  }
  else {
    $cleanupErrors = @($NativeProcess.TerminationError, $NativeProcess.StreamReadError) | Where-Object { $_ }
    "NETWORK_LANTERN_IPERF3_CLEANUP_FAILED run=$RunId process=$($NativeProcess.ProcessId): $($cleanupErrors -join ' ')"
  }
  $exception = if ($cleanupVerified) {
    [System.OperationCanceledException]::new($message)
  }
  else {
    [System.InvalidOperationException]::new($message)
  }
  $exception.Data['NetworkLantern.CleanupRecordVersion'] = 1
  $exception.Data['NetworkLantern.RunId'] = $RunId
  $exception.Data['NetworkLantern.CancellationObserved'] = $true
  $exception.Data['NetworkLantern.CleanupVerified'] = $cleanupVerified
  $exception.Data['NetworkLantern.RootExited'] = [bool]$NativeProcess.RootExited
  $exception.Data['NetworkLantern.TreeTerminationVerified'] = [bool]$NativeProcess.TreeTerminationVerified
  $exception.Data['NetworkLantern.StreamsCompleted'] = [bool]$NativeProcess.StreamsCompleted
  $exception.Data['NetworkLantern.TerminationScope'] = [string]$NativeProcess.TerminationScope
  $exception.Data['NetworkLantern.ProcessId'] = [int]$NativeProcess.ProcessId
  $exception.Data['NetworkLantern.UnterminatedProcessIds'] = [int[]]@($NativeProcess.UnterminatedProcessIds)
  $exception.Data['NetworkLantern.Error'] = [string](@($NativeProcess.TerminationError, $NativeProcess.StreamReadError) | Where-Object { $_ } | Join-String -Separator ' ')
  return $exception
}

function Get-Iperf3CompletedStreamText {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]
    [System.Threading.Tasks.Task]$StdOutTask,
    [Parameter(Mandatory)]
    [System.Threading.Tasks.Task]$StdErrTask,
    [ValidateRange(1, 30000)]
    [int]$TimeoutMs = 1000
  )
  $combinedTask = [System.Threading.Tasks.Task]::WhenAll([System.Threading.Tasks.Task[]]@($StdOutTask, $StdErrTask))
  $completed = $false
  $streamError = $null
  try {
    $null = $combinedTask.Wait($TimeoutMs)
    $completed = $StdOutTask.IsCompletedSuccessfully -and $StdErrTask.IsCompletedSuccessfully
  }
  catch {
    $completed = $false
    $streamError = $_.Exception.Message
  }
  $stdout = ''
  $stderr = ''
  if ($completed) {
    try { $stdout = [string]$StdOutTask.Result } catch { $streamError = $_.Exception.Message }
    try { $stderr = [string]$StdErrTask.Result } catch { $streamError = $_.Exception.Message }
  }
  elseif (-not $streamError) {
    $streamError = "Redirected streams did not close within ${TimeoutMs}ms. A descendant may still hold a pipe handle."
  }
  return [pscustomobject]@{
    Completed = $completed
    StdOut    = $stdout
    StdErr    = $stderr
    Error     = $streamError
  }
}

function Invoke-Iperf3NativeProcess {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param(
    [Parameter(Mandatory)]
    [string]$FilePath,
    [Parameter(Mandatory)]
    [AllowEmptyCollection()]
    [string[]]$Arguments,
    [Parameter(Mandatory)]
    [ValidateRange(1, 3660000)]
    [int]$TimeoutMs,
    [ValidateRange(1, 30000)]
    [int]$TerminationGracePeriodMs = 5000,
    [ValidateRange(1, 30000)]
    [int]$StreamDrainTimeoutMs = 1000,
    [string]$CancellationFile = $env:NETWORK_LANTERN_IPERF3_CANCEL_FILE,
    [string]$CancellationRunId = $env:NETWORK_LANTERN_IPERF3_CANCEL_RUN_ID,
    [string]$CancellationNonce = $env:NETWORK_LANTERN_IPERF3_CANCEL_NONCE,
    [ValidateRange(10, 1000)]
    [int]$CancellationPollIntervalMs = 100
  )
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $FilePath
  $Arguments | ForEach-Object { $psi.ArgumentList.Add([string]$_) }
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.UseShellExecute = $false
  $psi.CreateNoWindow = $true
  $proc = $null
  try {
    $proc = [System.Diagnostics.Process]::Start($psi)
    if (-not $proc) { throw "Failed to start native process: $FilePath" }
    $processId = $proc.Id
    # Start both readers before waiting so neither redirected pipe can fill.
    $stderrTask = $proc.StandardError.ReadToEndAsync()
    $stdoutTask = $proc.StandardOutput.ReadToEndAsync()
    $cancelled = $false
    $hasCancellationContract = -not [string]::IsNullOrWhiteSpace($CancellationFile) -and
      $CancellationRunId -match '^[a-fA-F0-9]{32}$' -and
      $CancellationNonce -match '^[a-fA-F0-9]{32}$'
    $expectedCancellationContent = if ($hasCancellationContract) {
      "NETWORK-LANTERN-IPERF3-CANCEL/1:${CancellationRunId}:${CancellationNonce}"
    } else { $null }
    if (-not $hasCancellationContract) {
      $completed = $proc.WaitForExit($timeoutMs)
    }
    else {
      $completed = $false
      $waitStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
      while (-not $completed -and $waitStopwatch.ElapsedMilliseconds -lt $TimeoutMs) {
        try {
          if (Test-Path -LiteralPath $CancellationFile -PathType Leaf -ErrorAction Stop) {
            $signalFileInfo = Get-Item -LiteralPath $CancellationFile -ErrorAction Stop
            if ($signalFileInfo.Length -gt 512) {
              Write-Verbose "Ignoring oversized iperf3 cancellation signal for run '$CancellationRunId'."
            }
            else {
              $signalContent = [System.IO.File]::ReadAllText($CancellationFile)
              if ([string]::Equals($signalContent, $expectedCancellationContent, [StringComparison]::Ordinal)) {
                $cancelled = $true
                break
              }
              Write-Verbose "Ignoring foreign or stale iperf3 cancellation signal for run '$CancellationRunId'."
            }
          }
        }
        catch {
          Write-Verbose "Could not inspect iperf3 cancellation signal '$CancellationFile': $($_.Exception.Message)"
        }
        $remainingMs = $TimeoutMs - [int]$waitStopwatch.ElapsedMilliseconds
        $waitSliceMs = [math]::Min($CancellationPollIntervalMs, [math]::Max($remainingMs, 1))
        $completed = $proc.WaitForExit($waitSliceMs)
      }
      $waitStopwatch.Stop()
    }
    $timedOut = (-not $completed -and -not $cancelled)
    $terminationSucceeded = $null
    $terminationError = $null
    $rootExited = [bool]$proc.HasExited
    $treeTerminationVerified = $false
    $terminationScope = 'NotRequested'
    $descendantProcessIds = [int[]]@()
    $unterminatedProcessIds = [int[]]@()
    if ($timedOut -or $cancelled) {
      $termination = Stop-Iperf3ProcessAfterTimeout -Process $proc -GracePeriodMs $TerminationGracePeriodMs
      $terminationSucceeded = [bool]$termination.TerminationSucceeded
      $terminationError = $termination.Error
      $rootExited = [bool]$termination.RootExited
      $treeTerminationVerified = [bool]$termination.TreeTerminationVerified
      $terminationScope = [string]$termination.TerminationScope
      $descendantProcessIds = [int[]]@($termination.DescendantProcessIds)
      $unterminatedProcessIds = [int[]]@($termination.UnterminatedProcessIds)
    }
    $rootExited = [bool]$proc.HasExited
    $exitCode = if ($rootExited) { $proc.ExitCode } else { -1 }
    # A normally exited root can still have a descendant holding inherited pipe
    # handles. Drain only within a fixed budget and never await these tasks later.
    $streamResult = Get-Iperf3CompletedStreamText -StdOutTask $stdoutTask -StdErrTask $stderrTask -TimeoutMs $StreamDrainTimeoutMs
    return [pscustomobject]@{
      ExitCode             = $exitCode
      StdOut               = $streamResult.StdOut
      StdErr               = $streamResult.StdErr
      StreamsCompleted     = [bool]$streamResult.Completed
      StreamReadError      = $streamResult.Error
      TimedOut             = $timedOut
      Cancelled            = $cancelled
      TerminationSucceeded = $terminationSucceeded
      TerminationError     = $terminationError
      RootExited           = $rootExited
      TreeTerminationVerified = $treeTerminationVerified
      TerminationScope     = $terminationScope
      DescendantProcessIds = $descendantProcessIds
      UnterminatedProcessIds = $unterminatedProcessIds
      ProcessId            = $processId
      CancellationRunId    = $CancellationRunId
    }
  }
  finally {
    if ($proc) { $proc.Dispose() }
  }
}
