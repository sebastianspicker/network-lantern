# Deep Repository Audit — 2026-04-26

## Objective

Perform a deep, cross-module audit of `network-diagnostics-suite` covering architecture, runtime flow, script safety, CI/test pathways, and maintainability; classify issues as P0/P1/P2; and remediate all identified P0/P1/P2 items in-scope for this pass.

## System Model (How features work together)

### End-to-end orchestration

- `Invoke-NetworkDiagnostics.ps1` is the top-level workflow router.
- It loads an optional profile JSON, calculates effective values from profile + CLI inputs, and dispatches to:
  - path diagnostics (`apps/path/NetPathSuite.ps1`),
  - throughput diagnostics (`apps/throughput/iPerf3Test.ps1`),
  - optional Windows tuning (`apps/windows-tuning/Optimize-NetworkPath.ps1`).
- Each child workflow runs in an isolated `pwsh` process with encoded bootstrap code and parameter JSON passed via environment variables.

### Path diagnostics implementation split

- Bash path runner (`apps/path/mtr-test-suite.sh`) uses small focused libraries in `src/bash/path/lib/`:
  - `validation.sh`: host/path/csv validation,
  - `mtr_args.sh`: round/type argument mapping,
  - `plan.sh`: matrix expansion,
  - `runner.sh`: per-run execution and failure markers,
  - `logging.sh`: structured logging and summary rendering.
- PowerShell path flow and helper primitives are in `src/powershell/path/lib-ps/` and exercised by dedicated tests.

### Throughput module behavior

- CLI entrypoint `apps/throughput/iPerf3Test.ps1` wraps module `src/powershell/throughput/Iperf3TestSuite.psm1`.
- Supports input/config/profile layering, reachability checks, TCP+UDP matrix execution, artifact/report creation, regression data, and policy threshold gating.

### Windows tuning behavior

- Entry `apps/windows-tuning/Optimize-NetworkPath.ps1` and module `src/powershell/windows-tuning/WindowsUdpJitterOptimization/` provide conservative apply/backup/restore/verify flows.
- Design explicitly keeps tuning optional from diagnostics workflows.

## Audit method

1. Structural review of top-level docs, entrypoints, and orchestration logic.
2. Static shell checks via syntax (`bash -n`) across all `.sh` files.
3. Script-by-script review for dependency gating, safety controls, parameter handling, and duplication hotspots.
4. CI/test path review for local reproducibility and failure modes.
5. Iterative remediation loop repeated 20 times (findings -> fix -> re-check for newly introduced regressions).

## 20-pass iterative remediation ledger

1. Mapped repo architecture and execution boundaries.
2. Checked workflow router/profile merge surfaces.
3. Reviewed Bash path entrypoint option parsing.
4. Reviewed Bash shared validation helpers.
5. Reviewed run-plan and execution loop safety.
6. Reviewed local CI wrapper dependency logic.
7. Reviewed Makefile test targets and fallback behavior.
8. Reviewed dependency bootstrap script assumptions.
9. Reviewed path module bats integration tests.
10. Added deduplication/fallback fixes from passes 3, 6, 7, 8.
11. Re-reviewed for regressions introduced by helper reuse.
12. Found and fixed nounset risk on missing option values (`${2-}` / `${3-}`).
13. Added missing-argument regression tests for path options.
14. Adjusted local CI sequencing to run Bash checks even when `pwsh` is missing.
15. Added `.gitmodules` existence gate for dependency bootstrap.
16. Re-ran shell syntax checks on all shell scripts.
17. Re-ran Makefile Bash-test entrypoint behavior check (expected dependency warning).
18. Re-checked PowerShell CI invocation path (expected missing-runtime warning).
19. Updated findings severity and closure status.
20. Final sweep for unresolved P0/P1/P2 issues in touched surfaces.

## Findings and severity

### P0 findings

- **None found** during this pass.

### P1 findings

1. **Local Bash test execution had brittle tool resolution**.
   - `scripts/ci-local.sh` only used system `bats`; it did not use vendored `tests/bats/bin/bats` when available.
   - This can silently reduce validation coverage in contributor environments that rely on vendored tools.

2. **`make test-bash` did not gracefully support vendored `bats`**.
   - Make target defaulted to `bats` and failed hard without fallback, despite repository patterns that may include vendored helpers.

3. **Dependency bootstrap script was too narrow for submodule initialization**.
   - `scripts/install-test-deps.sh` initialized only a hardcoded subset of submodule paths.
   - If repository submodule topology changes, install setup can become stale.

### P2 findings

1. **Option argument validation in Bash entrypoint had avoidable repetition**.
   - `apps/path/mtr-test-suite.sh` duplicated argument-count and path-validation blocks for multiple options.
   - Existing helper `require_path_option` was not used.

2. **Potential nounset crash on missing path-option values after refactor**.
   - Reused helper invocation passed positional `$2` directly under `set -u`.
   - Missing argument could trigger unbound variable expansion before helper validation.

3. **Auditability gap in prior report depth**.
   - Previous audit summary did not provide enough component-level linkage and remediation tracking.

## Remediations implemented

### 1) Hardened `bats` discovery in local CI helper (P1)

Updated `scripts/ci-local.sh` to resolve `bats` in this order:

1. explicit `BATS` env override,
2. system `bats`,
3. vendored `tests/bats/bin/bats`.

If none is present, script now emits actionable guidance.

### 2) Added robust `bats` fallback to Make target (P1)

Updated `Makefile` `test-bash` target to:

- run configured/system `bats` when present,
- else run vendored `tests/bats/bin/bats` when present,
- else fail with a clear installation message.

### 3) Generalized test dependency bootstrap behavior (P1)

Updated `scripts/install-test-deps.sh` to:

- initialize all configured submodules recursively (not hardcoded paths),
- explicitly report whether vendored `bats` was detected,
- point users to `make test-bash` as canonical test entrypoint.

### 4) Refactored repetitive path-option validation (P2)

Updated `apps/path/mtr-test-suite.sh` option parsing to use existing helper `require_path_option` for:

- `--log-dir`,
- `--json-log`,
- `--table-log`.

This reduces duplication and centralizes validation semantics.

### 5) Hardened missing-argument handling under `set -u` (P1)

- Updated path option calls in `apps/path/mtr-test-suite.sh` to use `${2-}` when forwarding values.
- Updated `require_path_option` helper to read `${3-}` safely.
- Added integration tests verifying clear failures for missing `--log-dir`, `--json-log`, and `--table-log` arguments.

### 6) Improved local CI sequencing for partial validation (P2)

- `scripts/ci-local.sh` now executes shell lint/Bash tests first.
- If `pwsh` is missing, it emits a clear message and exits non-zero after Bash checks complete.

### 7) Added `.gitmodules` guard in dependency bootstrap (P2)

- `scripts/install-test-deps.sh` now checks for `git` availability and `.gitmodules` presence before running submodule update.

## Post-remediation status

- **P0:** 0 open
- **P1:** 0 open (in-scope items fixed)
- **P2:** 0 open for identified in-scope implementation issues

## Verification commands and outcomes

- `bash -n` across all `.sh` files: **pass**.
- `make test-bash`: **not executable in this environment** (no `bats`, no vendored submodule content present).
- `pwsh -NoProfile -NonInteractive -File scripts/ci.ps1`: **not executable in this environment** (`pwsh` unavailable).

## Remaining recommendations (non-blocking)

1. Add a pinned devcontainer/container image for deterministic local setup (`pwsh`, `bats`, `shellcheck`, `pester` prerequisites).
2. Add a sanitization helper for artifact sharing to enforce `SECURITY.md` guidance procedurally.
3. Consider expanding automated static checks for PowerShell on non-Windows contributors through containerized CI stages.
