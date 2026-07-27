# Documentation

Start with [README.md](../README.md) for project scope, requirements,
configuration, common commands, repository layout, and operational limits.

## Operator guides

- [Choose a workflow](workflows/decision-tree.md)
- [Run path diagnostics](workflows/diagnose-path.md)
- [Run throughput diagnostics](workflows/diagnose-throughput.md)
- [Inspect or change Windows tuning](workflows/windows-tuning.md)
- [Review the Windows tuning scope](evidence/tuning-matrix.md)

## Design and development

- [Architecture](architecture.md)
- [Verification](verification.md)
- [Contribution guide](../CONTRIBUTING.md)
- [Security policy](../SECURITY.md)

## Migration

- [Network Diagnostics Suite](migration/from-network-diagnostics-suite.md)
- [mtr-test-suite](migration/from-mtr-test-suite.md)
- [iperf3-test-suite](migration/from-iperf3-test-suite.md)
- [windows-udp-jitter-optimization](migration/from-windows-udp-jitter-optimization.md)

Migration pages document only current compatibility mappings. Release history
is in [CHANGELOG.md](../CHANGELOG.md). The repository is licensed under the
[MIT License](../LICENSE).

## Local files

Generated diagnostics and development artifacts belong in ignored locations:

- `artifacts/`, `logs/`, `reports/`, `results/`, and `backups/`
- `.iperf3/` and `profiles/*.local.json`
- `.cache/` and tool-specific ignored directories

Do not move operational output into a tracked path without reviewing it for
hostnames, addresses, local paths, user data, credentials, and machine details.
