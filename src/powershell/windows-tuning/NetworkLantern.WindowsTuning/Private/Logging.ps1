function Write-UjInformation {
  [CmdletBinding()]
  [OutputType([void])]
  param(
    [Parameter(Mandatory)]
    [string]$Message
  )

  Write-Information -MessageData $Message -InformationAction Continue
}
