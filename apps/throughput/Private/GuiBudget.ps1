# Pure GUI budget binding helpers, kept separate for cross-platform tests.
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'The setter updates one transient GUI control only.')]
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '', Justification = 'MaxTotalTests is the established public parameter name represented by these bindings.')]
param()

function Get-GuiMaxTotalTests {
  [CmdletBinding()]
  [OutputType([int])]
  param([Parameter(Mandatory)][object]$Form)
  $control = $Form.Controls.Find('numMaxTotalTests', $true) | Select-Object -First 1
  if (-not $control) { return 0 }
  return [int]$control.Value
}

function Set-GuiMaxTotalTests {
  [CmdletBinding()]
  param(
    [Parameter(Mandatory)][object]$Form,
    [ValidateRange(0, 1000000)][int]$Value
  )
  $control = $Form.Controls.Find('numMaxTotalTests', $true) | Select-Object -First 1
  if ($control) { $control.Value = $Value }
}
