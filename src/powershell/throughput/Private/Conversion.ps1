# DSCP and bandwidth conversion helpers (private to NetworkLantern.Throughput)

function Get-TosFromDscpClass {
  [CmdletBinding()]
  [OutputType([int])]
  param(
    [Parameter(Mandatory)]
    [string]$Class
  )
  # DSCP values occupy bits 2-7 of the ToS/Traffic Class byte.
  # Left-shift by 2 converts a 6-bit DSCP value to the 8-bit ToS value that iperf3 -S expects.
  switch -Regex ($Class) {
    '^CS([0-7])$' {
      $cs = [int]$Matches[1]
      $dscp = 8 * $cs          # CS classes are multiples of 8 (RFC 2474)
      return ($dscp -shl 2)
    }
    '^EF$' { return (46 -shl 2) }  # EF = DSCP 46 = ToS 184
    '^AF([1-4])([1-3])$' {
      $x = [int]$Matches[1]    # class (1-4)
      $y = [int]$Matches[2]    # drop precedence (1-3)
      $dscp = (8 * $x) + (2 * $y)  # RFC 2597
      return ($dscp -shl 2)
    }
    default { throw "Unknown DSCP class: '$Class'. Expected CS0-CS7, EF, or AFxy (e.g. AF11, AF41)." }
  }
}

function ConvertTo-MbitPerSecond {
  [CmdletBinding()]
  [OutputType([double])]
  param(
    [Parameter(Mandatory)]
    [string]$Value
  )
  if ([string]::IsNullOrWhiteSpace($Value)) { throw "ConvertTo-MbitPerSecond: empty or whitespace input" }
  $m = [regex]::Match($Value.Trim(), '^(?<n>[0-9]+(\.[0-9]+)?)\s*(?<u>[kKmMgG])?$')
  if (-not $m.Success) {
    throw "Invalid bandwidth format: '$Value' (expected e.g. 500K, 10M, 1G)."
  }
  $n = [double]$m.Groups['n'].Value
  $u = $m.Groups['u'].Value.ToLowerInvariant()
  if ([string]::IsNullOrEmpty($u)) {
    Write-Warning "No unit suffix provided for bandwidth value '$Value'; assuming Mbps. Use K, M, or G suffix explicitly."
    $u = 'm'
  }
  switch ($u) {
    'g' { return [math]::Round($n * 1000, 3) }
    'k' { return [math]::Round($n / 1000, 3) }
    'm' { return [math]::Round($n, 3) }
    default { return [math]::Round($n, 3) }
  }
}
