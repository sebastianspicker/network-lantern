@{
  RootModule = 'NetworkLantern.Throughput.psm1'
  ModuleVersion = '0.5.0'
  GUID = '82181be0-e98a-4cd6-8b08-fbd6a29f1626'
  Author = 'Network Lantern contributors'
  CompanyName = ''
  Copyright = 'Copyright (c) 2025 sebastianspicker'
  Description = 'PowerShell iperf3 throughput measurement with profiles, DSCP matrices, thresholds, and result comparison.'
  PowerShellVersion = '7.0'
  CompatiblePSEditions = @('Core')
  FunctionsToExport = @(
    'Measure-NetworkThroughput',
    'Get-NetworkThroughputDefaultParameterSet',
    'Get-Iperf3ProfileNames',
    'Get-Iperf3ProfileParameters',
    'Save-Iperf3Profile',
    'Remove-Iperf3Profile',
    'Compare-Iperf3Runs'
  )
  CmdletsToExport = @()
  VariablesToExport = @()
  AliasesToExport = @()
  PrivateData = @{
    PSData = @{
      Tags = @('iperf3','network','benchmark')
      ReleaseNotes = 'Network Lantern rename and alpha documentation. ModuleVersion remains the component compatibility version.'
    }
  }
}
