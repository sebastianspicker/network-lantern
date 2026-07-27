# JSON extraction and metric helpers (private to NetworkLantern.Throughput)

function New-Iperf3Metric {
  [CmdletBinding()]
  [OutputType([pscustomobject])]
  param()
  [pscustomobject]@{ TxMbps = $null; RxMbps = $null; Retr = $null; LossPct = $null; JitterMs = $null }
}

function Get-BitsPerSecondMbps {
  [CmdletBinding()]
  [OutputType([object])]
  param(
    [AllowNull()]
    [object]$Obj
  )
  if (-not $Obj -or $Obj.PSObject.Properties.Name -notcontains 'bits_per_second') { return $null }
  try {
    $bps = [double]$Obj.bits_per_second
  }
  catch {
    return $null
  }
  [math]::Round($bps / 1e6, 2)
}

function Get-JsonSubstringOrNull {
  [CmdletBinding()]
  [OutputType([string])]
  param(
    [Parameter(Mandatory)]
    [string]$Text
  )
  if ([string]::IsNullOrWhiteSpace($Text)) { return $null }
  $maxLen = $script:MaxJsonTextLength
  if ($Text.Length -gt $maxLen) {
    Write-Warning "iperf3 JSON output truncated from $($Text.Length) to $maxLen bytes"
    $Text = $Text.Substring(0, $maxLen)
  }
  $first = $Text.IndexOf('{')
  $last = $Text.LastIndexOf('}')
  if ($first -ge 0 -and $last -gt $first) {
    $candidate = $Text.Substring($first, $last - $first + 1)
    try {
      $null = ConvertFrom-Json -InputObject $candidate -ErrorAction Stop
      return $candidate
    }
    catch { Write-Verbose "Broad JSON extraction failed; falling back to deep scan." }
  }
  # Deep scan: character-by-character brace-depth tracker that respects JSON double-quoted strings.
  # Needed when iperf3 output contains non-JSON text interspersed with partial JSON fragments.
  $depth = 0
  $start = -1
  $inString = $false
  $escape = $false
  $quote = [char]0
  $i = 0
  while ($i -lt $Text.Length) {
    $c = $Text[$i]
    if ($inString) {
      if ($escape) { $escape = $false }
      elseif ($c -eq '\') { $escape = $true }
      elseif ($c -eq $quote) { $inString = $false }
      $i++
      continue
    }
    if ($c -eq '"') { $inString = $true; $quote = $c; $i++; continue }
    if ($c -eq '{') { if ($depth -eq 0) { $start = $i }; $depth++; $i++; continue }
    if ($c -eq '}') {
      $depth--
      if ($depth -eq 0 -and $start -ge 0) {
        $candidate = $Text.Substring($start, ($i - $start + 1))
        try {
          $null = ConvertFrom-Json -InputObject $candidate -ErrorAction Stop
          return $candidate
        }
        catch { Write-Verbose "JSON candidate at position $start failed to parse." }
      }
      $i++
      continue
    }
    $i++
  }
  return $null
}
