# Changelog

## Unreleased

### Repository hygiene

- Keep mutable throughput profiles in ignored local state instead of a tracked
  JSON store.
- Exclude internal audit work, assistant plans, local indexes, operational
  output, packet captures, and tuning exports from the public repository.
- Make the local secret scan inspect Git publication candidates so ignored tool
  state and non-source sockets do not break verification.
- Align contributor, CI, security, architecture, and operator documentation
  with `main` as the current integration branch and the current local layout.

## v1.0.0 — 2026-04-18

### Added

- Orchestration (`Invoke-NetworkDiagnostics.ps1`): JSON-serialised isolated child-process launch; resolves array argument misbinding when calling downstream scripts.
- Orchestration: direct regression tests covering Triage, Path, and Baseline workflows.
- Path: per-stage status fields (`PingStatus`, `TracertStatus`, `PathpingStatus`, `Tcp443Status`, `PortsStatus`) and a derived `OverallStatus` (`OK`/`Fail`) on every result row.
- Path: `FailedStages` list on each result; process exit code now reflects any stage failure, not only TCP/443.
- Path: `PathpingStatus = Skipped` when `-SkipPathping` is used — distinct from `OK`.
- Path: `Get-RoundDefinitions` accepts explicit execution parameters instead of reading ambient caller scope.
- Path: `Invoke-HostDiagnostics` and `Invoke-DiagnosticsMatrix` accept an explicit `Settings` object, eliminating ambient-variable coupling.
- Bash path: `DEFAULT_TEST_TYPES=(ICMP4 ICMP6 TCP4 TCP6)` and `DEFAULT_ROUNDS=(Standard)` replace the previous full-matrix default.
- Bash path: empty `--types`, `--rounds`, `--hosts4`, `--hosts6` values are now rejected as input errors.
- Throughput: structured `try/catch` around CLI bootstrap so import failures exit with code 12 (Prerequisite).
- Throughput: CLI precedence refactored — `$forwardParams` carries only user-provided or config values; module defaults are applied inside the module.
- Throughput: IPv6 validation replaced the permissive regex with `[System.Net.IPAddress]::TryParse`; scope-id addresses (`%`) are rejected.
- Windows tuning: `Get-UjManagedQosPolicy` `ErrorOnFailure` switch; `Verify` now returns `Success = False` and `Unknown` component status when QoS enumeration is unavailable.
- Windows tuning: `Read-UjBackupManifest` validates schema version and tool name before any restore work; incompatible manifests yield `Warn`/`Skipped` without destructive side-effects.
- Windows tuning: `Get-UjBackupManifestMetadata` enriches backup manifests with machine name, OS version, platform, module version, and schema version.
- Windows tuning: `UjBackupSchemaVersion = 2` and `UjOwnedMmcssAudioValueNames` constant for scope-bounded reset.
- Windows tuning: `Reset-UjBaseline` removes only owned MMCSS Audio values instead of deleting entire task keys.
- Windows tuning: new tests for verify false-success, incompatible manifest rejection, and reset scope.
- Docs: workflow docs and architecture updated to reflect corrected behaviour and public surface.

### Changed

- Windows tuning module: `Invoke-UdpJitterOptimization` and `Get-UjDefaultBackupFolder` removed from `FunctionsToExport`.
- `apps/windows-tuning/Optimize-NetworkPath-GUI.ps1`: changed from `Write-Error` to an informational `Write-Output` redirect.
- `Compare-Iperf3Runs` documentation narrowed to status/counts/timing comparison.
- `NetPathSuite.ps1` error message updated: `NetTestSuite` → `NetPathSuite`.
- `Qos.ps1` `New-UjDscpPolicyByApp`: fixed broken warning format string.

### Initial repo work (carried forward from dev)

- repurposed the archived Windows UDP jitter repo into `network-diagnostics-suite`
- imported path diagnostics from `mtr-test-suite`
- imported throughput diagnostics from `iperf3-test-suite`
- replaced the public Windows tuning surface with a conservative CLI-first workflow
- added umbrella orchestration, workflow docs, migration notes, and unified CI on `dev`
