# Changelog

## Unreleased

### Repository reconstruction

- Restore regular file modes: only the seven directly invoked shell scripts are
  executable, and the architecture test now rejects any other executable file.
- Give process statuses one home (`contracts::exit`) and derive the run state
  from a status in one place. CLI exit codes are unchanged: CLI tests pin the
  parse and validation statuses, and runtime tests pin interruption.
- Fix: a desktop run that fails before writing a summary now reports status `1`
  for non-throughput capabilities, matching its summary and the CLI, instead of
  the throughput status table.
- Split the Rust runtime's application layer into request, planning, execution,
  errors, workflow, profile, and report modules with no module cycles, and make
  internal modules crate-private; the summary record is a typed struct
  with a golden key test.
- Replace the compressed desktop frontend with focused TypeScript modules. DOM
  structure, command payloads, and screenshots match the previous interface.
- Pin run states and error categories shared by Rust and the desktop in
  `tests/fixtures/contracts/wire-enums.json`.
- Use one ShellCheck list (`make lint`) and one Pester entrypoint
  (`scripts/ci.ps1 -Filter`); remove `scripts/Invoke-Tests.ps1`,
  `scripts/Get-RepoRoot.ps1`, the rename-era identity guard, the retired-path
  test, and the benchmark harnesses that could not run from a clean checkout.
- Check that default path targets agree across all five sources, run the
  architecture tests in Rust CI, and test the planner before Pages deploys it.
- Remove unused Rust dependencies (`chrono` in helper, `thiserror` in path-trace,
  `libc` in runtime).

### Architecture reconstruction

- Add explicit `NetworkLantern.Path` and `NetworkLantern.Workflow` modules and
  reduce their public scripts to parameter and exit-status adapters.
- Make the Bash MTR feature load through one composition root and move CLI
  orchestration out of the public shell entrypoint.
- Split throughput exports into `Public/`, keep implementation in `Private/`,
  and remove runtime dependencies on development scripts and private paths.
- Replace the compact cross-capability test file with capability-owned behavior
  suites and mechanical dependency-boundary checks.
- Preserve supported execution entrypoints, module exports, profile/report
  formats, exit codes, locking, cancellation, and Windows restore compatibility.

### Alpha release preparation

- Define `0.1.0-alpha.1` as the first unified repository release candidate.
- Document the alpha compatibility boundary, platform limits, runtime
  requirements, configuration precedence, local state, and troubleshooting.
- Distinguish normal throughput run previews from explicit profile save and
  delete operations, which modify the selected profile store.
- Align contribution, security, verification, issue, and pull request guidance
  with the current validation commands.

### Public interface rename

- Rename the project to Network Lantern and use `network-lantern` as the
  intended repository slug.
- Rename active operator commands to `Invoke-NetworkLantern.ps1`,
  `Test-NetworkPath.ps1`, `test-network-path.sh`,
  `Measure-NetworkThroughput.ps1`, `Measure-NetworkThroughput-GUI.ps1`, and
  `Invoke-NetworkPathTuning.ps1`.
- Remove the obsolete tuning GUI guidance stub; the tuning CLI is the sole
  supported tuning entrypoint.
- Rename the throughput and Windows tuning modules to
  `NetworkLantern.Throughput` and `NetworkLantern.WindowsTuning`.
- Rename exported tuning helpers to `Get-NetworkLanternDefaultBackupFolder`
  and `Test-NetworkTuningAdministrator`.

### Windows safety and recovery

- Make tuning `-DryRun` non-mutating and non-elevated for Apply, Backup, and
  Restore previews. Real Apply, Backup, and Restore remain elevation-gated.
- Remove the public administrator-check bypass from the tuning command and
  exported function.
- Require a complete, verified backup before Apply reaches any tuning mutation.
- Advance backup manifests to schema 3 with artifact digests, backup path trust
  checks, protected restore staging, and revalidation before restore consumers.
- Isolate Windows restore fixtures with trusted disposable access controls.
- Replace host `netsh` execution in the reset-scope test with a test double that
  verifies the intended calls.

### Verification and portability

- Load exactly PSScriptAnalyzer 1.24.0 and Pester 5.7.1 in local and CI gates.
- Fail full and filtered Pester gates when no tests are selected or executed.
- Replace the Bash test suite's Python JSON dependency with `jq`.
- Normalize text files to LF through `.gitattributes`.
- Add a read-only prerequisite report and document the cross-shell and
  PowerShell-only verification paths.

### Repository hygiene

- Keep mutable throughput profiles in ignored local state instead of a tracked
  JSON store.
- Keep machine-local state, operational output, packet captures, and tuning
  exports outside version control.
- Ignore Python bytecode caches, pytest caches, and TypeScript build metadata.
- Make the local secret scan inspect tracked and non-ignored untracked files.
- Align maintained documentation with `main` as the integration branch and the
  current source layout.

### Portability and presentation

- Replace locale-specific default path targets with globally neutral public
  services (`cloudflare.com`, `google.com`, `wikipedia.org`, `amazon.com`) in the
  Bash, PowerShell, and Rust path defaults and in `config/hosts.conf`.
- Publish the static command planner to GitHub Pages through a dedicated
  workflow.
- Add a static-planner screenshot tour to the README.

### Implementation structure

- Split throughput native-process, invocation, metric, and test-execution
  helpers into responsibility-specific private files with explicit load order.
- Split Windows backup, manifest validation, restore staging, component
  restore, and action orchestration into responsibility-specific private files.
- Split throughput implementation responsibilities behind an explicit module
  load order.

## Pre-alpha development snapshot (2026-04-18)

This snapshot was previously labeled `v1.0.0`, but no matching Git tag exists
locally or remotely. It is kept as development history, not a published stable
release.

### Added

- Added isolated child-process orchestration with serialized array arguments.
- Added direct regression tests for Triage, Path, and Baseline workflows.
- Added per-stage path status fields and a derived overall status for each
  result row.
- Added failed-stage reporting and made path process status reflect any planned
  stage failure.
- Distinguished skipped pathping work from successful pathping work.
- Removed ambient caller-scope dependencies from path round and diagnostic
  helpers.
- Changed the Bash default to the bounded
  `ICMP4,ICMP6,TCP4,TCP6` by `Standard` matrix.
- Rejected empty Bash type, round, and host selections.
- Added structured throughput CLI exit handling for initialization failures.
- Moved throughput defaults into the module and kept CLI forwarding limited to
  explicitly supplied or configured values.
- Replaced permissive IPv6 validation with `IPAddress.TryParse` and rejected
  scope identifiers.
- Made Windows tuning verification report unavailable QoS enumeration as an
  unknown component and a failed verification.
- Added Windows backup schema validation, metadata, and scope-bounded reset
  behavior.

### Changed

- Removed legacy Windows tuning helpers from the exported module surface.
- Changed the legacy tuning GUI entrypoint from an error to an informational
  CLI redirect.
- Narrowed `Compare-Iperf3Runs` documentation to the fields it compares.
- Corrected path and QoS error messages.

### Source consolidation

- Consolidated code from the prior Network Diagnostics Suite, MTR path,
  `iperf3` throughput, and Windows UDP jitter workspaces.
- Replaced the earlier Windows tuning surface with an optional CLI workflow.
- Added umbrella orchestration, workflow documentation, migration notes, and a
  unified CI configuration.
