# network-diagnostics-suite

Operator-facing network diagnostics for path analysis, throughput testing, and optional Windows endpoint tuning.

The repository contains reusable source, tests, examples, and public operator
documentation. Generated diagnostics, saved targets, machine-specific indexes,
and internal work notes stay local and are ignored by Git.

## Modules

- `path`: route, loss, latency, TCP reachability, and best-effort UDP probes
- `throughput`: `iperf3` TCP/UDP throughput matrices, thresholds, and regression artifacts
- `windows-tuning`: optional, conservative Windows QoS and endpoint policy adjustments with backup and restore

## Quick start

Path diagnostics:

```powershell
pwsh -File .\apps\path\NetPathSuite.ps1 -DryRun -Protocols IPv4
```

Throughput baseline:

```powershell
pwsh -File .\apps\throughput\iPerf3Test.ps1 -Target iperf3.example.net -SingleTest
```

Workflow orchestration:

```powershell
pwsh -File .\Invoke-NetworkDiagnostics.ps1 -Workflow Triage -DryRun
pwsh -File .\Invoke-NetworkDiagnostics.ps1 -Workflow Throughput -IperfTarget iperf3.example.net
pwsh -File .\Invoke-NetworkDiagnostics.ps1 -Workflow WindowsTuning -TuningAction Apply -TuningProfile Safe -DryRun
```

## Verification

Prerequisite readiness check, no installs:

```powershell
pwsh -File .\scripts\Test-Prerequisites.ps1
```

Local suite, no dependency installs:

```bash
./scripts/ci-local.sh
```

PowerShell-only checks:

```powershell
pwsh -File .\scripts\ci.ps1
```

PowerShell-only checks without installing missing modules:

```powershell
pwsh -File .\scripts\ci.ps1 -NoInstall
```

The complete local gate requires PowerShell 7, Git, ShellCheck, Bats, and
Python 3.
Throughput runs additionally require `iperf3`; the Bash path workflow requires
`mtr`, `jq`, and `column`. Applying Windows tuning requires Windows and may
require administrator rights, while `-DryRun` remains non-destructive.

Windows notes:

- Bash and Bats files are pinned to LF line endings through `.gitattributes`
  so WSL Bash can parse them from a Windows checkout.
- `scripts\ci.ps1` installs pinned PowerShell test modules when they are
  missing unless `-NoInstall` is used.
- Use `pwsh -File .\scripts\Test-Prerequisites.ps1 -IncludeIperf3` when
  validating a host for real throughput runs.
- `iPerf3Test.ps1 -WhatIf`, `NetPathSuite.ps1 -DryRun`, and Windows tuning
  `-DryRun` are the safe preview modes; real throughput and path runs can
  perform network probes.

## Layout

```text
apps/
  path/
  throughput/
  windows-tuning/
src/
  bash/path/
  powershell/path/
  powershell/throughput/
  powershell/windows-tuning/
tests/
  path/
  throughput/
  windows-tuning/
docs/
  architecture.md
  workflows/
  evidence/
  migration/
profiles/
config/
```

`profiles/example-office.json` is a sanitized workflow example. Runtime
throughput profiles are written to ignored local state: direct module use
defaults to `.iperf3/profiles.json`, while umbrella orchestration uses
`profiles/throughput-profiles.local.json`.

## Documentation

- Architecture: [docs/architecture.md](docs/architecture.md)
- Documentation index: [docs/DOCUMENTATION.md](docs/DOCUMENTATION.md)
- Windows dry-run readiness: [docs/windows-dry-run-readiness.md](docs/windows-dry-run-readiness.md)
- Workflow guide: [docs/workflows/decision-tree.md](docs/workflows/decision-tree.md)
- Tuning evidence: [docs/evidence/tuning-matrix.md](docs/evidence/tuning-matrix.md)
- Migration notes: [docs/migration/from-windows-udp-jitter-optimization.md](docs/migration/from-windows-udp-jitter-optimization.md)

## Scope boundaries

- Diagnostics work without Windows tuning.
- Windows tuning is optional and CLI-first.
- Aggressive and folklore tweaks are intentionally excluded.
- `cs2-opt` remains separate; this repo only keeps a conservative, generic Windows tuning subset.

## Publication boundary

Do not commit diagnostic artifacts, raw routes, internal hostnames or IP
addresses, saved throughput targets, tuning backups, registry exports, packet
captures, or local tool state. `.gitignore` covers the supported local output
paths and analysis tools. Before publishing, run:

```bash
git status --short
pwsh -NoProfile -NonInteractive -File scripts/Invoke-SecretScan.ps1
```
