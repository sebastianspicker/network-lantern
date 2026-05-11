# Windows Dry-Run Readiness Review

Review date: 2026-05-11

This companion report records a safe Windows dry-run pass for the network diagnostics suite. The scripts were originally authored on macOS but target Windows operators, so this review focuses on local Windows checkout behavior, non-mutating dry-run commands, and fix proposals. No system dependencies were installed, no tuning changes were applied, and no real network probes were run. Temporary test dependencies used for verification were isolated under the ignored `.cache/` directory.

## Implementation Status

The follow-up fix pass implemented the highest-value items from this review:

- Added `.gitattributes` rules to keep text files checked out with LF endings, matching `.editorconfig`.
- Renormalized tracked text files to LF.
- Added `scripts/Test-Prerequisites.ps1` for non-mutating readiness checks.
- Added `scripts/ci.ps1 -NoInstall` to prevent module installation during PowerShell validation.
- Updated README verification notes for Windows and dry-run usage.
- Suppressed unselected protocol host summaries in path dry-run output.
- Updated local test scripts so safe verification does not require unavailable submodules, does not scan ignored dependency caches, and imports the requested Pester version before using Pester 5 types.

## Environment Snapshot

| Item | Observed state |
| --- | --- |
| Repository state before documentation | `main...origin/main`, clean |
| Host shell | Windows PowerShell launching commands from `C:\Users\sebastian\Desktop\git\network-diagnostics-suite` |
| PowerShell | `pwsh` available at `C:\Program Files\WindowsApps\Microsoft.PowerShell_7.6.1.0_x64__8wekyb3d8bbwe\pwsh.exe` |
| Bash | WSL launcher available at `C:\Windows\system32\bash.exe` |
| Initial missing CLI tools on PATH | `shellcheck`, `bats`, `iperf3` |
| Initial PowerShell modules | `PSScriptAnalyzer` not installed; `Pester 3.4.0` installed, but repo CI expects `Pester 5.7.1` |
| Line ending policy | `.editorconfig` requires `end_of_line = lf` |
| Initial Git line ending state | Bash-related files were indexed as LF but checked out as CRLF (`git ls-files --eol`) |
| Git attributes | `.gitattributes` now enforces LF for text files and marks common binary extensions as binary |

## Initial Safe Dry-Run Results

| Command | Exit code | Safety notes | Result summary |
| --- | ---: | --- | --- |
| `pwsh -NoProfile -NonInteractive -File .\apps\path\NetPathSuite.ps1 -DryRun -Protocols IPv4 -Rounds Standard -HostsIPv4 example.com -LogDirectory .\artifacts\dryrun-path` | 0 | Uses the script's dry-run mode; no probes executed and no output files created. | Planned 1 IPv4 `Standard` run for `example.com`; announced would-be JSON/CSV paths. |
| `pwsh -NoProfile -NonInteractive -File .\apps\throughput\iPerf3Test.ps1 -Target example.com -WhatIf -SkipReachabilityCheck -DisableMtuProbe -OutDir .\artifacts\dryrun-throughput` | 0 | Uses `-WhatIf`, skips reachability and MTU probes; does not require `iperf3` execution. | Reported approximately 1110 planned tests and would-be CSV/JSON paths. |
| `pwsh -NoProfile -NonInteractive -File .\apps\windows-tuning\Optimize-NetworkPath.ps1 -Action Apply -TuningProfile Safe -DryRun -SkipAdminCheck -PassThru` | 0 | Uses `-DryRun` and skips admin check; no registry, QoS, backup, or power-plan writes. | Returned `Success = True`; components were marked skipped. |
| `pwsh -NoProfile -NonInteractive -File .\Invoke-NetworkDiagnostics.ps1 -Workflow Triage -DryRun -Protocols IPv4 -Rounds Standard -HostsIPv4 example.com -OutRoot .\artifacts\dryrun-orchestration` | 0 | Orchestrates the path dry run in an isolated child PowerShell process. | Planned 1 path run and announced would-be artifact paths under `artifacts/dryrun-orchestration/path`. |
| `bash -n scripts/ci-local.sh` | 1 | Syntax-only Bash parse; no script body execution. | Failed with `syntax error: unexpected end of file`, consistent with CRLF checkout. |
| `bash -n scripts/install-test-deps.sh` | 0 | Syntax-only Bash parse; no submodule or dependency action. | Parsed successfully despite CRLF checkout. |
| `bash -n scripts/run-workflow.sh` | 1 | Syntax-only Bash parse; no workflow execution. | Failed with `syntax error: unexpected end of file`, consistent with CRLF checkout. |
| `bash -n apps/path/mtr-test-suite.sh` | 1 | Syntax-only Bash parse; no diagnostics executed. | Failed near `usage() {` with a visible carriage return token. |
| PowerShell AST parse across `*.ps1`, `*.psm1`, and `*.psd1` | 0 | Parser-only validation; no scripts executed. | Parsed 67 PowerShell files with no syntax errors. |

## Initial Issues And Fix Proposals

| Severity | Area | Reproduction | Impact | Proposed fix |
| --- | --- | --- | --- | --- |
| High | Bash line endings on Windows checkout | `bash -n apps/path/mtr-test-suite.sh` | WSL Bash cannot parse several Bash entrypoints when the Windows working tree uses CRLF. | Add `.gitattributes` rules for `*.sh`, `*.bash`, and `*.bats` with `text eol=lf`, then renormalize affected files. |
| High | Local CI dependency clarity | `Get-Command shellcheck,bats,iperf3` | Local verification fails or is skipped depending on which tools are installed; operators do not get a single non-mutating readiness report. | Add a prerequisite check script or `-CheckPrerequisites` mode that reports missing tools without installing anything. |
| Medium | PowerShell module readiness | `pwsh -File .\scripts\Invoke-Tests.ps1` | `Invoke-Tests.ps1` fails because only legacy `Pester 3.4.0` is installed; `PSScriptAnalyzer` is absent. | Document Windows setup commands and/or add a non-mutating module check before running test scripts. |
| Medium | `scripts/ci.ps1` side effects | Inspect `scripts/ci.ps1` or run only with installation approved. | The script installs exact module versions when missing, so it is not a pure dry-run validation command. | Split dependency installation from validation or add an explicit no-install mode. |
| Low | Path dry-run summary noise | Run path dry run with `-Protocols IPv4`. | Summary still prints configured IPv6 hosts even though no IPv6 runs are selected. | Suppress unselected protocol host summaries or label them as configured-but-unused. |
| Low | Platform-specific setup docs | Compare README local suite instructions with Windows tool availability. | `./scripts/ci-local.sh` is advertised as the local suite but assumes Unix tooling and LF checkout behavior. | Add separate Windows local setup and macOS/Linux setup notes. |

## Prioritized Fix Proposals

1. Add `.gitattributes` with LF enforcement for Bash and Bats files, then renormalize the affected files. This is the smallest change that addresses the immediate Windows checkout breakage.
2. Add a non-mutating readiness check for required tools and modules: `pwsh`, `bash`, `git`, `shellcheck`, `bats`, `iperf3`, `Pester 5.7.1`, and `PSScriptAnalyzer 1.24.0`.
3. Document Windows local verification separately from macOS/Linux verification, including the distinction between dry runs, dependency installation, and real network probes.
4. Consider a `scripts/ci.ps1` no-install mode so users can validate readiness without modifying their PowerShell module environment.
5. Clean up path dry-run output so protocol-specific runs only print host summaries relevant to the selected protocol.

## Follow-Up Verification

After implementing line-ending fixes, re-run:

```powershell
bash -n scripts/ci-local.sh
bash -n scripts/install-test-deps.sh
bash -n scripts/run-workflow.sh
bash -n apps/path/mtr-test-suite.sh
```

After implementing dependency setup or a readiness checker, verify it reports missing tools without installing them unless explicitly requested.

Run the repo's intended full CI only after dependency installation is approved, or use `scripts/ci.ps1 -NoInstall` to validate with already-available modules.

## Final Safe Verification

The final pass used ignored sandbox dependencies under `.cache/` and did not install global/system tools, perform Windows tuning writes, or run real diagnostics probes.

| Check | Result |
| --- | --- |
| `pwsh -File .\scripts\Test-Prerequisites.ps1` with sandbox `PATH` and `PSModulePath` | Pass |
| `pwsh -File .\scripts\ci.ps1 -NoInstall` | Pass; Pester 5.7.1 reported 212 passed, 0 failed, 3 skipped |
| `.\scripts\ci-local.sh` via Git Bash with sandbox `shellcheck` and `bats` | Pass; delegates to `scripts/ci.ps1 -NoInstall` |
| `pwsh -File .\scripts\Invoke-QualityGates.ps1` with sandbox `PSModulePath` | Pass; delegates to `scripts/ci.ps1 -NoInstall` |
| Safe PowerShell app dry runs: path, throughput `-WhatIf`, Windows tuning `-DryRun`, orchestration triage `-DryRun` | Pass |
| Safe Bash dry runs: `scripts/install-test-deps.sh`, `scripts/run-workflow.sh`, `apps/path/mtr-test-suite.sh --dry-run` | Pass |
| Git line ending check for CRLF working-tree files after normalization | Pass; no `w/crlf` entries remain |

Remaining out-of-scope validation: real `iperf3` throughput runs, real path probes, and non-dry-run Windows tuning were intentionally not executed because they mutate the host or network state.
