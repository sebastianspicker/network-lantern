# Throughput diagnostics

Throughput runs use PowerShell 7 and `iperf3` 3.7 or newer. A live run requires
a trusted or operator-controlled `iperf3` server.

## Preview and quick run

Preview a single TCP test without checking `iperf3`, connecting to the target,
or creating output:

```powershell
pwsh -NoProfile -File .\apps\throughput\Measure-NetworkThroughput.ps1 `
  -Target iperf3.example.net -SingleTest -WhatIf
```

Replace the reserved example hostname before a live run:

```powershell
pwsh -NoProfile -File .\apps\throughput\Measure-NetworkThroughput.ps1 `
  -Target iperf3.example.net -SingleTest -Summary
```

`-SingleTest` runs one transmit test. With `-Protocol UDP`, it runs one UDP
transmit test; otherwise it runs one TCP transmit test.

## Matrix behavior

The default values are:

| Parameter | Default |
| --- | --- |
| `Port` | `5201` |
| `Duration` | 10 seconds |
| `Omit` | 1 second |
| `Protocol` | `Both` |
| `IpVersion` | `Auto` |
| `DscpClasses` | `CS0,AF11,CS5,EF,AF41` |
| `TcpStreams` | `1,4,8` |
| `TcpWindows` | `default,128K,256K` |
| `UdpStart` | `1M` |
| `UdpMax` | `1G` |
| `UdpStep` | `10M` |
| `UdpLossThreshold` | 5 percent |
| `ConnectTimeoutMs` | 60000 |

For TCP, the module tests transmit and receive directions and adds
bidirectional tests when the installed `iperf3` supports them. For UDP, it runs
fixed transmit and receive tests plus a bandwidth ramp in both directions.
The ramp stops for a DSCP class and direction after measured loss exceeds
`UdpLossThreshold`.

With the defaults and bidirectional support, `-WhatIf` reports approximately
1,110 tests. Restrict the protocol, DSCP classes, stream counts, window sizes,
or UDP range before running a matrix on a shared network.

Example TCP-only matrix:

```powershell
pwsh -NoProfile -File .\apps\throughput\Measure-NetworkThroughput.ps1 `
  -Target iperf3.example.net -Protocol TCP `
  -DscpClasses CS0,EF -TcpStreams 1,4 -TcpWindows default -Progress -Summary
```

## Connectivity checks

On Windows, the default live sequence checks ICMP reachability, probes MTU
payload sizes, and checks the configured TCP server port before running tests.
`-SkipReachabilityCheck` skips the ICMP check only. It does not skip the TCP
port check.

On Linux and macOS, use the direct entrypoint with both platform-specific probe
switches:

```powershell
pwsh -NoProfile -File ./apps/throughput/Measure-NetworkThroughput.ps1 `
  -Target iperf3.example.net -SingleTest `
  -SkipReachabilityCheck -DisableMtuProbe
```

The orchestrator does not expose these two switches, so its live Throughput,
Baseline, and throughput-enabled Triage workflows currently require Windows.

## Thresholds and exit codes

Use `-ThresholdMinThroughputMbps`, `-ThresholdMaxLossPct`, and
`-ThresholdMaxJitterMs` to classify a completed run. Thresholds are disabled by
default.

| Code | Meaning |
| ---: | --- |
| 0 | success, plan preview, or profile listing |
| 11 | input or configuration validation failure |
| 12 | missing or incompatible prerequisite |
| 13 | connectivity check failure |
| 14 | partial test failure or threshold breach |
| 15 | all tests failed |
| 16 | internal or unclassified failure |

The orchestrator returns the child exit code unchanged.

## Configuration and profiles

`-ConfigurationPath` reads JSON settings. `-ProfileName` loads a named profile.
Precedence is:

1. explicit command-line parameter
2. JSON configuration
3. named profile
4. module default

Unknown or invalid values warn and are ignored unless
`-StrictConfiguration` is supplied.

Named profiles use `.iperf3/profiles.json` in the current directory by default.
Examples:

```powershell
pwsh -NoProfile -File .\apps\throughput\Measure-NetworkThroughput.ps1 `
  -Target 192.0.2.10 -ProfileName lab -SaveProfile

pwsh -NoProfile -File .\apps\throughput\Measure-NetworkThroughput.ps1 `
  -ListProfiles

pwsh -NoProfile -File .\apps\throughput\Measure-NetworkThroughput.ps1 `
  -DeleteProfile lab
```

The documentation address `192.0.2.10` is not a live server. Profile save and
delete commands modify the selected file. `-SaveProfile -WhatIf` still writes,
and delete does not honor `-WhatIf`.

## Output and comparison

Direct live runs default to `./logs` and write timestamped result CSV and JSON,
a summary JSON document, a Markdown report, and
`iperf3_run_index.json`. `-Force` permits timestamped output collisions to be
overwritten. `-OpenOutputFolder` opens the selected output directory after a
run on a supported desktop platform.

Compare two generated summary JSON files:

```powershell
Import-Module .\src\powershell\throughput\NetworkLantern.Throughput.psd1
Compare-Iperf3Runs `
  -BaselinePath .\logs\previous_summary.json `
  -CurrentPath .\logs\current_summary.json
```

The comparison reports changes in status, test count, failure count, and
elapsed time. It does not calculate a throughput regression from individual
test measurements.

## Windows Forms client

Launch the GUI on Windows:

```powershell
pwsh -NoProfile -File .\apps\throughput\Measure-NetworkThroughput-GUI.ps1
```

The GUI supports run planning, live execution, cancellation, profile
management, and output paths. Its helper and cancellation code has automated
coverage. Interactive layout, live server behavior, and end-to-end
cancellation have not been verified by automated tests.

## Troubleshooting

- Exit 12 usually indicates PowerShell, `iperf3`, or a platform probe mismatch.
- Exit 13 indicates a failed target or TCP port check. Confirm the target,
  port, firewall, and server process.
- Use `-WhatIf` to inspect test count before a full matrix.
- Use `-SingleTest` for a basic client and server check.
- On Linux and macOS, include both `-SkipReachabilityCheck` and
  `-DisableMtuProbe`.
