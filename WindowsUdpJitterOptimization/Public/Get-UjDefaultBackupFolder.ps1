function Get-UjDefaultBackupFolder {
  <#
  .SYNOPSIS
    Returns the default backup folder path used by the module (ProgramData\UDPTune or fallback).
  .OUTPUTS
    [string] Default backup folder path.
  #>
  [CmdletBinding()]
  [OutputType([string])]
  param()

  return $script:UjDefaultBackupFolder
}
