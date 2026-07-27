function Test-PathUnderBase {
  [CmdletBinding()]
  [OutputType([bool])]
  param(
    [Parameter(Mandatory)]
    [string]$BasePath,
    [Parameter(Mandatory)]
    [string]$CandidatePath
  )
  $baseFull = [System.IO.Path]::GetFullPath($BasePath)
  $candidateFull = [System.IO.Path]::GetFullPath($CandidatePath)
  $separators = @([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
  $baseWithSeparator = $baseFull.TrimEnd($separators) + [System.IO.Path]::DirectorySeparatorChar
  $comparison = if ($IsWindows) { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }
  return $candidateFull.Equals($baseFull, $comparison) -or $candidateFull.StartsWith($baseWithSeparator, $comparison)
}
