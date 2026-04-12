function Get-NdsDefaultBackupFolder {
  <#
  .SYNOPSIS
    Returns the default backup folder path used by the network-diagnostics-suite tuning module.
  .OUTPUTS
    [string]
  #>
  [CmdletBinding()]
  [OutputType([string])]
  param()

  return $script:UjDefaultBackupFolder
}
