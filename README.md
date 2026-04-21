# network-diagnostics-suite

Operator-facing network diagnostics for path analysis, throughput testing, and optional Windows endpoint tuning.

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

Local suite:

```bash
./scripts/ci-local.sh
```

PowerShell-only checks:

```powershell
pwsh -File .\scripts\ci.ps1
```

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

## Documentation

- Architecture: [docs/architecture.md](docs/architecture.md)
- Workflow guide: [docs/workflows/decision-tree.md](docs/workflows/decision-tree.md)
- Tuning evidence: [docs/evidence/tuning-matrix.md](docs/evidence/tuning-matrix.md)
- Migration notes: [docs/migration/from-windows-udp-jitter-optimization.md](docs/migration/from-windows-udp-jitter-optimization.md)

## Scope boundaries

- Diagnostics work without Windows tuning.
- Windows tuning is optional and CLI-first.
- Aggressive and folklore tweaks are intentionally excluded.
- `cs2-opt` remains separate; this repo only keeps a conservative, generic Windows tuning subset.
