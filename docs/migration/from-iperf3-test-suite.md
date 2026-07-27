# Migrating from iperf3-test-suite

The throughput implementation is now the `NetworkLantern.Throughput` module.

| Previous surface | Current surface |
| --- | --- |
| `iPerf3Test.ps1` | `apps/throughput/Measure-NetworkThroughput.ps1` |
| `iPerf3Test-GUI.ps1` | `apps/throughput/Measure-NetworkThroughput-GUI.ps1` |
| `Iperf3TestSuite` | `src/powershell/throughput/NetworkLantern.Throughput.psd1` |
| standalone use only | direct entrypoint or `Invoke-NetworkLantern.ps1 -Workflow Throughput` |

Direct named profiles default to `.iperf3/profiles.json`. The orchestrator uses
`profiles/throughput-profiles.local.json` internally and does not expose profile
save, load, list, or delete operations.

Use `-WhatIf` for a direct plan preview or umbrella `-DryRun` through the
orchestrator. A normal plan preview makes no connection and creates no result
files. Profile management is different: `-SaveProfile -WhatIf` writes the
profile, and `-DeleteProfile` changes the store.

Live Linux and macOS runs require the direct entrypoint with both
`-SkipReachabilityCheck` and `-DisableMtuProbe`. The TCP server-port check still
runs.

The CLI uses exit codes 11 through 16 for input, prerequisite, connectivity,
partial, total, and internal failures. The orchestrator preserves those codes.
