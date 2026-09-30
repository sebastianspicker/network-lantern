# Throughput diagnostics

Throughput runs use PowerShell 7 and `iperf3` 3.7 or newer. A live run needs a
trusted or operator-controlled `iperf3` server.

## Preview and quick run

Preview a single TCP test. This does not connect to the target or create output,
though local `iperf3` version and capability discovery may still run:

```powershell
pwsh -NoProfile -File .\apps\throughput\Measure-NetworkThroughput.ps1 `
  -Target iperf3.example.net -SingleTest -WhatIf
```

Replace the reserved example hostname before a live run:

```powershell
pwsh -NoProfile -File .\apps\throughput\Measure-NetworkThroughput.ps1 `
  -Target iperf3.example.net -SingleTest -Summary
```

`-SingleTest` runs one transmit test: UDP with `-Protocol UDP`, otherwise TCP.

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
| `MaxTotalTests` | `0` (unlimited) |

For TCP, the module tests transmit and receive directions, and adds
bidirectional tests when the installed `iperf3` supports them. For UDP, it runs
fixed transmit and receive tests plus a bandwidth ramp in both directions. The
ramp stops for a DSCP class and direction once measured loss exceeds
`UdpLossThreshold`.

With the defaults and bidirectional support, `-WhatIf` reports a planned maximum
of 1,145 tests, or 1,100 without bidirectional support. Before running a matrix
on a shared network, narrow the protocol, DSCP classes, stream counts, window
sizes, or UDP range.

`TotalApprox` keeps its name for compatibility and represents the maximum before
UDP loss-based early stopping. Ramp size is the lower integer number of steps
plus the initial measurement. When the lower and upper bounds are equal, the
ramp is skipped and one fixed measurement remains per DSCP class and direction.
Repeated matrix entries are intentional measurements.

Set `-MaxTotalTests` to an integer from 1 through 1,000,000 to reject a live plan
that exceeds that many measurements; the default, `0`, is unlimited. The budget
counts planned measurements and excludes retries, and equality is allowed. A
rejected plan exits 11 before connectivity checks, output-directory creation, or
measurements, though local capability discovery may still occur.

Over-budget `-WhatIf` previews remain available. They include `MaxTotalTests`,
`WithinTestBudget`, and `EstimatedTestSeconds`. The estimate is nominal: planned
measurements multiplied by `Duration + Omit`, excluding setup and retries and
allowing UDP early stopping. Default estimates are 12,595 seconds with
bidirectional support or 12,100 seconds without it.

The root workflow exposes the same limit as `-ThroughputMaxTotalTests`, and a
workflow JSON profile uses `throughput.maxTotalTests`. Explicit workflow CLI
values override profile values, including an explicit `0`.

Example TCP-only matrix:

```powershell
pwsh -NoProfile -File .\apps\throughput\Measure-NetworkThroughput.ps1 `
  -Target iperf3.example.net -Protocol TCP `
  -DscpClasses CS0,EF -TcpStreams 1,4 -TcpWindows default -Progress -Summary
```

## Connectivity checks

On Windows, the default live sequence checks ICMP reachability, probes MTU payload
sizes, and checks the configured TCP server port before running tests.
`-SkipReachabilityCheck` skips the ICMP check only; it does not skip the TCP port
check.

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

`-ConfigurationPath` reads JSON settings, and `-ProfileName` loads a named
profile. Values resolve in this order:

1. explicit command-line parameter
2. JSON configuration
3. named profile
4. module default

Unknown or invalid values warn and are ignored unless you pass
`-StrictConfiguration`.

Named profiles use `.iperf3/profiles.json` in the current directory by default:

```powershell
pwsh -NoProfile -File .\apps\throughput\Measure-NetworkThroughput.ps1 `
  -Target 192.0.2.10 -ProfileName lab -SaveProfile -WhatIf

pwsh -NoProfile -File .\apps\throughput\Measure-NetworkThroughput.ps1 `
  -ListProfiles

pwsh -NoProfile -File .\apps\throughput\Measure-NetworkThroughput.ps1 `
  -DeleteProfile lab
```

For a small named TCP matrix, save the existing profile mechanism's parameters
together with a budget, then preview that profile before running it:

```powershell
pwsh -NoProfile -File .\apps\throughput\Measure-NetworkThroughput.ps1 `
  -Target 192.0.2.10 -ProfileName small-tcp -SaveProfile -WhatIf `
  -Protocol TCP -DscpClasses CS0 -TcpStreams 1 -TcpWindows default `
  -Duration 3 -Omit 0 -MaxTotalTests 3

pwsh -NoProfile -File .\apps\throughput\Measure-NetworkThroughput.ps1 `
  -ProfileName small-tcp -WhatIf
```

This plans at most three measurements (two without bidirectional support). The
save command includes `-WhatIf` to preview the matrix after saving instead of
running it; the usual precedence rules still apply.

The documentation address `192.0.2.10` is not a live server. Profile save and
delete modify the selected file: `-SaveProfile -WhatIf` still writes, and delete
does not honor `-WhatIf`.

## Output and comparison

Direct live runs default to `./logs` and write timestamped result CSV and JSON, a
summary JSON document, a Markdown report, and `iperf3_run_index.json`. `-Force`
lets a run overwrite a timestamped output collision. `-OpenOutputFolder` opens the
output directory after a run on a supported desktop platform.

Primary JSON, CSV, and Markdown artifacts each use an exclusive temporary file
beside their destination, flushed and closed before atomic replacement. A failed
write leaves the existing file complete. This guarantee is per file, with no
transaction across the set of run artifacts.

Native output capture drains stdout and stderr concurrently, retaining at most
1 MiB and 64 KiB respectively and discarding excess bytes until the streams
finish. Exceeding either limit fails that measurement, even if the retained
prefix parses as JSON. Results expose `StdOutTruncated`, `StdErrTruncated`, and
`NativeOutputTruncated` to flag capture overflow.

Metrics are parsed before raw diagnostic retention is limited to 16 KiB of UTF-8
text per completed test. `RawTextTruncated` flags this shorter diagnostic text;
retention-only truncation does not fail valid measurements.

Compare two generated summary JSON files:

```powershell
Import-Module .\src\powershell\throughput\NetworkLantern.Throughput.psd1
Compare-Iperf3Runs `
  -BaselinePath .\logs\previous_summary.json `
  -CurrentPath .\logs\current_summary.json
```

The comparison reports changes in status, test count, failure count, and elapsed
time. It does not derive a throughput regression from individual measurements.

## Windows Forms client

Launch the GUI on Windows:

```powershell
pwsh -NoProfile -File .\apps\throughput\Measure-NetworkThroughput-GUI.ps1
```

Start with **Connection and run basics**, then set a maximum-test budget
(`0` means unlimited). Expand **Advanced test matrix** to change DSCP classes,
TCP windows and stream counts, or the UDP saturation ramp. Result thresholds stay
visible next to the budget.

Choose **Preview plan** to see the planned maximum, nominal duration, and budget
status returned by the throughput module. Editing settings marks the previous
estimate as out of date, so correct any highlighted fields before starting a
preview or run. **Run tests** starts live measurements; **Cancel** requests
cancellation and keeps the existing cleanup safeguards.

The **Profiles** tab loads and saves named settings, including zero-valued omit
time, loss thresholds, and an unlimited budget. The **Reports** tab shows the
latest run outcome and links to its artifacts. The window uses native resizing
and scrolling controls with accessible field names.

Validation, preview presentation, profile mapping, and cancellation helpers have
automated coverage. Native Windows layout, live server behavior, and end-to-end
cancellation still need separate Windows verification. See
[../TESTING.md](../TESTING.md) for the current gaps.

## Troubleshooting

- Exit 12 usually means a PowerShell, `iperf3`, or platform-probe mismatch.
- Exit 13 means the target or TCP port check failed. Confirm the target, port,
  firewall, and server process.
- Use `-WhatIf` to inspect the test count before a full matrix.
- Use `-SingleTest` for a basic client and server check.
- On Linux and macOS, include both `-SkipReachabilityCheck` and `-DisableMtuProbe`.
