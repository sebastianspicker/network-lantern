@{
  RootModule        = 'WindowsUdpJitterOptimization.psm1'
  ModuleVersion     = '3.0.0'
  GUID              = 'd73b26e7-18cb-49c7-9af3-f5d8fd6fa34c'
  Author            = 'Sebastian J. Spicker'
  CompanyName       = ''
  Copyright         = ''
  Description       = 'Conservative Windows network tuning helpers for network-diagnostics-suite.'
  PowerShellVersion = '7.0'
  FunctionsToExport = @('Invoke-NetworkPathTuning', 'Get-NdsDefaultBackupFolder', 'Invoke-UdpJitterOptimization', 'Get-UjDefaultBackupFolder', 'Test-UjIsAdministrator')
  CmdletsToExport   = @()
  VariablesToExport = @()
  AliasesToExport   = @()
  PrivateData       = @{
    PSData = @{
      Tags       = @('Windows', 'Networking', 'QoS', 'Diagnostics', 'Latency')
      LicenseUri = ''
      ProjectUri = ''
    }
  }
}
