# Documentation Index

These pages describe the supported public operator surface in the current
repository. Historical audits, remediation ledgers, local tool indexes, and
machine-specific verification notes belong under the ignored `docs/archive/`
workspace and are not part of the published documentation contract.

- Architecture: [architecture.md](architecture.md)
- Decision tree: [workflows/decision-tree.md](workflows/decision-tree.md)
- Path workflow: [workflows/diagnose-path.md](workflows/diagnose-path.md)
- Throughput workflow: [workflows/diagnose-throughput.md](workflows/diagnose-throughput.md)
- Windows tuning workflow: [workflows/windows-tuning.md](workflows/windows-tuning.md)
- Tuning evidence matrix: [evidence/tuning-matrix.md](evidence/tuning-matrix.md)
- Migration from legacy repos:
  - [from-iperf3-test-suite.md](migration/from-iperf3-test-suite.md)
  - [from-mtr-test-suite.md](migration/from-mtr-test-suite.md)
  - [from-windows-udp-jitter-optimization.md](migration/from-windows-udp-jitter-optimization.md)

## Local-only state

- `artifacts/`, `logs/`, `reports/`, `results/`, and `backups/`: generated or
  operational output
- `.iperf3/` and `profiles/*.local.json`: saved targets and profile state
- `.codegraph/`, `.serena/`, `.kilo/`, and similar tool directories: local
  indexes, plans, and assistant state
- `docs/archive/`: internal audit and remediation work products

Sanitize any operational evidence before moving it into a tracked public page.
