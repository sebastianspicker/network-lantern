@{
  RootModule        = 'NetworkLantern.WindowsTuning.psm1'
  ModuleVersion     = '3.0.0'
  GUID              = 'd26bb83e-2b42-488b-be25-8ffd108d3e5b'
  Author            = 'Sebastian J. Spicker'
  CompanyName       = ''
  Copyright         = 'Copyright (c) 2025 sebastianspicker'
  Description       = 'Optional Windows network policy verification, backup, apply, and restore helpers.'
  PowerShellVersion = '7.0'
  FunctionsToExport = @('Invoke-NetworkPathTuning', 'Get-NetworkLanternDefaultBackupFolder', 'Test-NetworkTuningAdministrator')
  CmdletsToExport   = @()
  VariablesToExport = @()
  AliasesToExport   = @()
  PrivateData       = @{
    PSData = @{
      Tags       = @('Windows', 'Networking', 'QoS', 'Diagnostics', 'Latency')
    }
  }
}
