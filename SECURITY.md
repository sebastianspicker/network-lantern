# Security

- Do not publish artifacts containing internal hostnames, IPs, routes, or local paths.
- Treat saved throughput reports and Windows tuning backups as sensitive operational data.
- Report security issues privately through the repository security contact or a private channel.
- Do not attach registry backups, `Get-NetQosPolicy` exports, or raw route traces to public issues unless sanitized.

## Defensive Review Notes

### Repo Index

- `Invoke-NetworkDiagnostics.ps1`: umbrella workflow dispatcher; merges profile defaults with CLI input and runs child PowerShell entrypoints in isolated processes.
- `apps/path/` and `src/powershell/path/`: Windows path diagnostics over `ping`, `tracert`, `pathping`, `Test-NetConnection`, and best-effort UDP/TCP probes.
- `apps/path/mtr-test-suite.sh` and `src/bash/path/`: Bash/MTR path diagnostics with host/config/path validation.
- `apps/throughput/` and `src/powershell/throughput/`: `iperf3` throughput tests, saved profiles, result reports, and run indexes.
- `apps/windows-tuning/` and `src/powershell/windows-tuning/`: optional Windows QoS/NIC/power-plan tuning with backup and restore.
- `scripts/`: local CI, PowerShell quality gates, helper functions, and lightweight secret scanning.
- `tests/`: Bats and Pester unit, integration, smoke, and E2E coverage for operator workflows.

### Threat Model

- Auth boundary: no network service or login surface exists in this repo; authorization is local operator control and OS account privileges.
- Input boundary: CLI parameters, JSON workflow profiles, throughput profile stores, `config/hosts.conf`, and environment-derived paths.
- File boundary: artifacts, logs, throughput profile JSON, Windows tuning backup folders, registry exports, CSV, CLIXML, and run reports.
- Network boundary: diagnostic targets are operator-supplied hosts passed to native tools; command invocation must stay argument-array based and reject shell/control characters.
- Privilege boundary: Windows tuning restore/apply can require administrator rights and can modify registry, QoS, NIC, and power-plan state.

### Findings

| ID | Severity | Status | Evidence | Impact | Patch |
| --- | --- | --- | --- | --- | --- |
| NDS-SEC-001 | Medium | Fixed on `security/remediate-review-findings` | `Restore-UjState` previously continued when `backup_manifest.json` was missing or malformed; regression tests now cover both cases in `tests/windows-tuning/WindowsNetworkTuning.Tests.ps1`. | A local attacker or accidental operator error could point restore at an untrusted folder and trigger registry/QoS/NIC/power restore helpers without a validated suite manifest. | Restore now fails closed for every non-OK manifest status before any component restore helper runs. |
| NDS-SEC-002 | Low | Fixed on `security/remediate-review-findings` | `Invoke-NetworkDiagnostics.ps1` read `-ProfilePath` with `Get-Content -Raw | ConvertFrom-Json` and no file-size limit. | A very large local profile could cause avoidable memory/CPU pressure during defensive tooling runs. | Workflow profiles now have a 1 MB cap before parsing, with Pester coverage for oversized files. |
| NDS-SEC-003 | Low | Fixed on `security/remediate-review-findings` | `Open-FolderOrFile` passed output paths directly to platform openers; throughput `OutDir` validation did not reject leading-dash path names. | A local operator-supplied path that looks like an option could change opener behavior when `-OpenOutputFolder` is used. | Throughput `OutDir` rejects option-like paths, opener calls receive absolute paths, and classification stays input-validation. |

### PR Summary

Title: `fix: remediate security review findings`

Summary:

- Block Windows tuning restore when the backup manifest is missing, invalid, or incompatible.
- Cap workflow profile size before JSON parsing.
- Reject option-like throughput output paths and pass absolute paths to platform openers.
- Document the current security review index, threat model, finding closure, and verification.

Verification:

- `pwsh -NoProfile -NonInteractive -Command "Invoke-Pester -Path ./tests/orchestration/Invoke-NetworkDiagnostics.Tests.ps1,./tests/throughput/Iperf3TestSuite.Tests.ps1,./tests/windows-tuning/WindowsNetworkTuning.Tests.ps1 -CI"`
- `./scripts/ci-local.sh`
- `git diff --check`
