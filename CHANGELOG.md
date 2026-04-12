# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [2.0.0] - Setting Reclassification

### Breaking Changes

- **Preset 3 no longer applies TCP-only, WoL-only, or unproven settings** (TCP checksum offload, LSO, RSC, ARP/NS offload, Wake-on-* settings, Jumbo Packet, Receive/Transmit Buffers, MMCSS Audio Task tuning). To get the old Preset 3 behavior, use `-IncludeExperimental`.
- `Set-UjMmcssAudioSafety` renamed to `Set-UjMmcssAudioTaskTuning` (private function; no public API change).
- `Set-UjUndocumentedNetworkMmcssTuning` replaced by `Set-UjNetworkThrottlingIndex` and `Set-UjSystemResponsiveness` (private functions; no public API change).

### Added

- **`-IncludeExperimental` switch** on `Invoke-UdpJitterOptimization`, CLI wrapper, and GUI. Applies TCP-only, WoL/sleep-only, and unproven settings on top of any preset.
- **Evidence-based setting classification** into Tier 1 (Safe), Tier 2 (Moderate), Tier 3 (Aggressive), and Experimental. See `docs/DOCUMENTATION.md` for the full classification table.
- **NIC keyword tier arrays** in Constants.ps1 (`UjNicKeywordsTier1/2/3/Experimental`) replace the flat keyword list.
- **Reverse keyword map** (`UjNicKeywordReverseMap`) for automatic display name resolution.
- `NetworkThrottlingIndex` moved from Preset 3 to Preset 2 (well-documented MMCSS setting).
- `SystemResponsiveness` now uses preset-dependent values: 20 (Preset 1), 10 (Preset 2), 0 (Preset 3).
- `*GreenEthernet` and `*PowerSavingMode` moved from Preset 2 to Preset 1 (same zero-risk category as EEE).
- GUI: "Include experimental" checkbox; updated preset labels to Safe/Moderate/Aggressive.
- Tests: NIC keyword tier overlap detection, reverse map coverage, `IncludeExperimental` in PassThru schema.
- Reset now cleans up `Tasks\Audio` registry key (written by experimental MMCSS Audio Task tuning).
- `IncludeExperimental` field in `-PassThru` result schema.

### Migration Guide

If you previously used **Preset 3** and relied on TCP offload disables, WoL disables, RSC disable, buffer tuning, or MMCSS Audio Task tuning, add `-IncludeExperimental` to get the same behavior:

```powershell
# Old (v1.x): Preset 3 applied everything
.\optimize-udp-jitter.ps1 -Action Apply -Preset 3

# New (v2.0): Preset 3 + experimental to match old behavior
.\optimize-udp-jitter.ps1 -Action Apply -Preset 3 -IncludeExperimental
```

Reset (`-Action ResetDefaults`) continues to reset ALL settings regardless of what was applied, including experimental settings.

## [1.1.0] - Previous Release

### Added

- `Invoke-UdpJitterOptimization` now supports `-PassThru` with structured result output for automation.
- `Invoke-UdpJitterOptimization` now supports `-AllowUnsafeBackupFolder` to explicitly override backup path safety checks.
- Restore component status model (`OK|Warn|Skipped`) across `Registry`, `Qos`, `NicAdvanced`, `Rsc`, and `PowerPlan`.
- New private action split files:
  - `Private/Actions.BackupRestore.ps1`
  - `Private/Actions.Apply.ps1`
  - `Private/Actions.Reset.ps1`
- New backup folder safety helper `Test-UjUnsafeBackupFolder`.
- New Pester coverage for PassThru schema, unsafe backup folder behavior, restore status mapping, and CLI default backup folder resolution.

### Changed

- Module loader (`WindowsUdpJitterOptimization.psm1`) now uses deterministic private/public script load order.
- CLI wrapper (`optimize-udp-jitter.ps1`) resolves default backup folder via module function `Get-UjDefaultBackupFolder` when not provided.
- GUI (`optimize-udp-jitter-gui.ps1`) now applies action-dependent control enablement, centralized input validation, and phased log output (`[Validate]`, action phase, `[Output]`, `[Done]`).
- Documentation consolidated aggressively to one technical document: `docs/DOCUMENTATION.md`.
- README reduced to quick operational entrypoint with a single technical docs link.

### Removed

- `testResults.xml` from repository tracking.
- Deprecated/duplicate technical docs:
  - `docs/BUGS-AND-FIXES.md`
  - `docs/INSPECTION-AND-FIXES.md`
  - `docs/plans/repo-and-code-improvements-plan.md`
- Legacy monolithic private action file `Private/Actions.ps1`.

### Fixed

- Restore summary now exposes full per-component status instead of registry-only output.
- Backup folder duplication removed from CLI default parameter expression.
- Dry-run Apply path no longer emits an empty information message.
- Backup folder unsafe-path detection now checks both raw and resolved canonical paths against sensitive roots (`Windows`, `System32`, `Program Files`).
- GUI run action now has explicit reentry protection to avoid accidental double-execution on rapid clicks.
