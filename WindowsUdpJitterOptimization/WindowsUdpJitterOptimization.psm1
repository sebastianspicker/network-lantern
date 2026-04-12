Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$privateDir = Join-Path -Path $PSScriptRoot -ChildPath 'Private'
$constantsPath = Join-Path -Path $privateDir -ChildPath 'Constants.ps1'
if (-not (Test-Path -LiteralPath $constantsPath)) {
  throw "Required module file missing: $constantsPath"
}
. $constantsPath

$privateLoadOrder = @(
  'Logging.ps1',
  'Filesystem.ps1',
  'Registry.ps1',
  'Platform.ps1',
  'Qos.ps1',
  'Nic.ps1',
  'Actions.BackupRestore.ps1',
  'Actions.Apply.ps1',
  'Actions.Reset.ps1'
)
foreach ($fileName in $privateLoadOrder) {
  $filePath = Join-Path -Path $privateDir -ChildPath $fileName
  if (-not (Test-Path -LiteralPath $filePath)) {
    throw "Required private module file missing: $filePath"
  }
  . $filePath
}

$publicDir = Join-Path -Path $PSScriptRoot -ChildPath 'Public'
$publicLoadOrder = @(
  'Get-UjDefaultBackupFolder.ps1',
  'Invoke-UdpJitterOptimization.ps1'
)
foreach ($fileName in $publicLoadOrder) {
  $filePath = Join-Path -Path $publicDir -ChildPath $fileName
  if (-not (Test-Path -LiteralPath $filePath)) {
    throw "Required public module file missing: $filePath"
  }
  . $filePath
}

Export-ModuleMember -Function 'Invoke-UdpJitterOptimization', 'Get-UjDefaultBackupFolder', 'Test-UjIsAdministrator'
