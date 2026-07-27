# Architecture

Network Lantern is divided by operator task. Entrypoints under `apps/` load
implementations under `src/`. Tests exercise both public scripts and internal
helpers.

| Area | Entrypoint | Implementation | Primary output |
| --- | --- | --- | --- |
| PowerShell path | `apps/path/Test-NetworkPath.ps1` | `src/powershell/path/lib-ps/` | JSON and CSV |
| Bash path | `apps/path/test-network-path.sh` | `src/bash/path/lib/` | JSON-object stream and text log |
| Throughput | `apps/throughput/Measure-NetworkThroughput.ps1` | `src/powershell/throughput/` | CSV, JSON, Markdown, and run index |
| Throughput GUI | `apps/throughput/Measure-NetworkThroughput-GUI.ps1` | throughput module plus Windows Forms | same files as the CLI |
| Windows tuning | `apps/windows-tuning/Invoke-NetworkPathTuning.ps1` | `src/powershell/windows-tuning/NetworkLantern.WindowsTuning/` | structured status and optional backup |
| Orchestration | `Invoke-NetworkLantern.ps1` | child entrypoints above | child workflow output |

## Path diagnostics

Both path entrypoints load default targets from `config/hosts.conf`. Command
line host arguments override the file. They share target configuration but not
their probe matrices or result schemas.

The PowerShell path entrypoint builds a matrix from rounds, IP protocols, and
hosts. Each live run uses Windows `ping`, `tracert`, `pathping`, and a TCP 443
check through `Test-NetConnection`. It records stage status and derives an
overall pass or failure for each planned run.

The Bash entrypoint builds a matrix from `mtr` test types, rounds, and hosts.
It invokes one `mtr` process at a time, enforces a per-process timeout, and
appends each result as a separate JSON value. Its `.json.log` output is a
stream of JSON objects, not a single JSON array.

## Throughput

`NetworkLantern.Throughput` exports:

- `Measure-NetworkThroughput`
- `Get-NetworkThroughputDefaultParameterSet`
- `Get-Iperf3ProfileNames`
- `Get-Iperf3ProfileParameters`
- `Save-Iperf3Profile`
- `Remove-Iperf3Profile`
- `Compare-Iperf3Runs`

The CLI wrapper merges defaults, JSON configuration, a named profile, and
explicit parameters. Explicit parameters have highest precedence. A test run
then validates its target and settings, checks prerequisites and connectivity,
probes `iperf3` capabilities, builds a test plan, executes native processes,
and writes reports.

Each native `iperf3` process has a deadline based on duration and omit values,
plus a 30-second buffer. The Windows Forms client runs tests in a background
job and uses a nonce-bound signal file for cancellation. Tests cover the
cancellation protocol and cleanup reporting, not the complete interactive UI.

Direct profile storage defaults to `.iperf3/profiles.json`. Profile writes use
a lock file and atomic replacement. Invalid JSON is rejected in strict mode.
In non-strict mode the module attempts to preserve a corrupt copy before using
an empty store.

## Windows tuning

`NetworkLantern.WindowsTuning` exports:

- `Invoke-NetworkPathTuning`
- `Get-NetworkLanternDefaultBackupFolder`
- `Test-NetworkTuningAdministrator`

`Verify` reads local QoS state. `Apply`, `Backup`, and `Restore` require Windows
and elevation unless the command is a dry run. Apply creates and validates a
backup before reaching any configuration mutation. Restore validates manifest
shape, schema compatibility, required artifacts, digests, and path trust. It
then copies artifacts to protected staging and revalidates that staging before
each component uses it.

The public profiles are intentionally narrow:

- `Safe` enables local QoS marking and creates requested UDP port or application
  DSCP policies.
- `Measured` adds supported NIC power-saving changes and selects the High
  Performance power plan unless another supported plan option is supplied.

See [the tuning evidence matrix](evidence/tuning-matrix.md) for included and
excluded settings.

## Orchestration

`Invoke-NetworkLantern.ps1` accepts an optional profile and maps one workflow to
one or more child scripts:

| Workflow | Child execution |
| --- | --- |
| `Path` | PowerShell path entrypoint |
| `Throughput` | throughput CLI; requires `IperfTarget` |
| `Baseline` | path, then one throughput test; requires `IperfTarget` |
| `WindowsTuning` | Windows tuning CLI |
| `Triage` | path, then throughput only when `IperfTarget` is present |

Children run in separate PowerShell processes. Parameters are serialized to
JSON through process-scoped environment variables. A nonzero child exit code is
returned unchanged by the orchestrator.

`scripts/run-workflow.sh` only checks for `pwsh` and forwards its arguments to
the orchestrator.

Umbrella `-DryRun` maps to path `-DryRun`, throughput `-WhatIf`, or tuning
`-DryRun`. Orchestrated path and throughput output goes to `path/` and
`throughput/` under `-OutRoot`, which defaults to `artifacts/`. Windows tuning
backup selection is independent of `-OutRoot`.
