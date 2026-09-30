# Testing and verification

Run every command from the repository root. Routine verification uses controlled
fakes and preview paths; it must not contact live targets or change Windows
settings.

## Toolchain

| Dependency | Version | Used by |
| --- | --- | --- |
| PowerShell | 7+ | Modules, scripts, and Pester |
| PSScriptAnalyzer | 1.24.0 | PowerShell static analysis |
| Pester | 5.7.1 | PowerShell behavior and architecture tests |
| Bash | 4+ for the product path CLI | Bash scripts and tests |
| ShellCheck | Not pinned | Bash static analysis |
| Bats | Not pinned | Bash tests |
| `jq` | Not pinned | Bash JSON assertions |
| Git | Not pinned | Prerequisite reporting and contributor checks |
| Node.js | 22+ | Static planner and desktop checks; development only |
| Rust | 1.96.0 | Pinned Cargo workspace and native desktop builds |

`iperf3`, `mtr`, and `column` are operator dependencies, not requirements for the
automated test gate.

Inspect the current environment without installing anything:

```powershell
pwsh -NoProfile -NonInteractive -File .\scripts\Test-Prerequisites.ps1
```

Add `-IncludeIperf3` to check that `iperf3` is on `PATH`. The prerequisite script
does not validate the Bash major version or the `iperf3` version; the live
entrypoints enforce those requirements.

`scripts/install-test-deps.sh` reports missing Bash tools and package-manager
commands. Despite its name, it installs nothing.

## The complete gate

From Bash, Git Bash, or WSL:

```bash
./scripts/ci-local.sh
```

This is the authoritative pre-completion gate. It runs `scripts/ci-legacy.sh`
and then `scripts/ci-rust.sh`; either can be run alone while iterating.

`scripts/ci-legacy.sh` first checks that `pwsh`, ShellCheck, Bats, `jq`, and
Node.js 22+ are available, then runs, in order:

1. `make lint`: ShellCheck (`-x`) over every tracked shell script. The
   `SHELL_SCRIPTS` list in the `Makefile` is the only ShellCheck file list.
2. The Bash path entrypoint contracts and all Bats tests under `tests/path/bash/`.
3. Node's built-in test runner over `tests/site/*.test.cjs`.
4. `scripts/Invoke-SecretScan.ps1`.
5. `scripts/ci.ps1 -NoInstall`, the only Pester entrypoint.

`scripts/ci.ps1` runs PSScriptAnalyzer and all Pester behavior and architecture
suites. It fails if analysis reports an issue, if Pester discovers no tests, or if
Pester returns a status other than `Passed`.

`scripts/ci-rust.sh` runs the Node architecture tests
(`tests/architecture/*.test.cjs`), the desktop and Rust checks described under
[Rust migration gate](#rust-migration-gate), and the whitespace check.

If the complete gate cannot run (for example, `pwsh` is unavailable), run the
checks that can (`make lint`, `make test-bash`, `scripts/ci-rust.sh`), and name
every skipped check and the resulting gap when reporting. A PowerShell-only run
of `scripts/ci.ps1 -NoInstall` is not equivalent to the gate.

By default, the gate installs no operating-system packages and no PowerShell
modules. Where current-user PowerShell module installation is allowed, run:

```bash
NETWORK_LANTERN_INSTALL_MISSING_MODULES=1 ./scripts/ci-local.sh
```

That option installs only the pinned PSScriptAnalyzer and Pester versions.

## PowerShell and focused checks

Run the PowerShell phase alone without installing dependencies:

```powershell
pwsh -NoProfile -NonInteractive -File .\scripts\ci.ps1 -NoInstall
```

Omit `-NoInstall` to install missing pinned PowerShell modules for the current
user. When needed, the script changes the PowerShell Gallery trust policy
temporarily and restores the previous value afterward.

Run a Pester subset while you iterate:

```powershell
pwsh -NoProfile -NonInteractive -File .\scripts\ci.ps1 -NoInstall `
  -Filter 'Throughput'
```

The filter matches Pester full names, skips PSScriptAnalyzer, and fails when it
selects no tests. Useful
filters include `Path`, `Throughput`, `Windows tuning`, and `Workflow`. A focused
run is not equivalent to the complete gate.

## Make targets

| Target | Scope |
| --- | --- |
| `make lint` | ShellCheck only |
| `make test-bash` | Bats path tests only |
| `make test-pwsh` | `scripts/ci.ps1 -NoInstall` |
| `make test-site` | Dependency-free static planner command and interaction tests |
| `make test` | Bats, static planner tests, then the PowerShell gate; omits ShellCheck and the secret scan |
| `make ci-local` | Complete cross-shell gate |

The legacy suite has no separate build or package step. The Rust and desktop
checks below add formatting, type checking, builds, and native UI tests. There is
no configured Markdown linter.

## Safe command previews

These examples validate planning without live probes or result-file writes:

```powershell
pwsh -NoProfile -File .\apps\path\Test-NetworkPath.ps1 `
  -HostsIPv4 example.com -Protocols IPv4 -Rounds Standard -DryRun

pwsh -NoProfile -File .\apps\throughput\Measure-NetworkThroughput.ps1 `
  -Target iperf3.example.net -SingleTest -WhatIf

pwsh -NoProfile -File .\Invoke-NetworkLantern.ps1 `
  -Workflow Triage -IperfTarget iperf3.example.net -DryRun

pwsh -NoProfile -File .\apps\windows-tuning\Invoke-NetworkPathTuning.ps1 `
  -Action Apply -TuningProfile Safe -UdpPorts 5201 -DryRun -PassThru
```

The Bash equivalent is:

```bash
./apps/path/test-network-path.sh \
  --hosts4 example.com --types ICMP4,TCP4 --rounds Standard --dry-run
```

Previews still read configuration and validate the inputs needed to build a plan.
A tuning dry run does not prove that an application path or a hardware-specific
setting will succeed during live Apply, and restore dry runs read and validate an
existing backup. Invalid inputs return a nonzero status.

Throughput profile save and delete are not previews: `-SaveProfile -WhatIf` still
writes the profile, and `-DeleteProfile` changes the selected store.

## Continuous integration

`.github/workflows/ci.yml` runs on pushes to `main`, pull requests targeting
`main`, and manual dispatch.

- `path-lint-test` runs the complete local gate on Ubuntu.
- `powershell-lint-test` runs the secret scan and PowerShell gate on Windows.

The workflow pins third-party actions by commit and caches PSScriptAnalyzer
1.24.0 and Pester 5.7.1.

`.github/workflows/rust.yml` supplies the matching Rust and desktop jobs, and
`.github/workflows/pages.yml` publishes the static planner in `site/` to GitHub
Pages. Neither workflow's configuration proves that remote CI has passed.

## What the tests cover

Automated tests cover plan construction, input and filesystem behavior, preview
non-mutation, exit codes, Bash timeouts and signal escalation, throughput profile
locking and cancellation contracts, workflow precedence and child isolation,
Windows tuning backup and restore defenses, and repository dependency rules.

Focused regressions cover:

- resolved workflow validation and child-process failure status;
- QoS inventory failure without any QoS mutation;
- bounded native throughput streams and UTF-8 diagnostics;
- plan cardinality and measurement budgets; and
- per-file atomic artifact replacement.

The static planner suite checks pure command generation across every workflow,
checkbox state, and select option, including input validation and budget
forwarding. A minimal DOM fixture exercises the shipped interaction script's
keyboard navigation, field errors, reset behavior, and clipboard success, failure,
and stale-command feedback. These Node tests do not render a browser, so layout
changes also need desktop and mobile browser QA.

GUI regressions cover text validation before the busy state, plan and result
presentation, zero-valued profile settings, and adapter control and event
contracts. They run without loading Windows Forms on non-Windows hosts.

## What the tests do not cover

- They do not contact the default public path targets.
- They do not run a live `iperf3` client against an external server.
- They do not visually test the Windows Forms interface.
- They do not exercise the iperf3 timeout implementation against real descendant
  process trees.
- They do not complete an elevated Windows tuning apply, injected failure, and
  restore cycle on a disposable VM.

Do not present these gaps as verified runtime behavior.

## Troubleshooting

- If `scripts/ci.ps1 -NoInstall` reports a missing pinned module, run
  `scripts/ci.ps1` once where current-user installation is allowed.
- On Windows, make sure Bash dependencies and `pwsh` are visible from the same Git
  Bash or WSL environment that runs `scripts/ci-local.sh`.
- If a focused run selects no tests, use a substring from a Pester `Describe`,
  `Context`, or `It` name.
- Preserve LF line endings and Git mode `100755` for executable shell entrypoints.

## Rust migration gate

`scripts/ci-rust.sh` is the second half of `scripts/ci-local.sh`. It requires Rust 1.96.0 and the desktop Node dependencies installed
with `npm ci` under `desktop/`. It runs:

- Node architecture tests, including default-host parity across implementations;
- Rust formatting and Clippy with warnings denied;
- workspace unit and integration tests;
- a CLI release build;
- desktop TypeScript and frontend tests;
- rendered Playwright flows and native WebdriverIO flows; and
- a production Tauri host and helper build.

The TypeScript checks cover the interface, test files, and test configuration.
Native tests include cancellation during window shutdown, and they verify the
persisted partial report before confirming process exit. The WebDriver launcher
passes an allowlist of platform environment variables so debug diagnostics cannot
log unrelated account credentials, and Linux headless native tests use Xvfb.
Install the development Chromium engine with `npx playwright install chromium`
from `desktop/` before the local gate.

The unchanged legacy suites continue to protect the behavior reference until
migration acceptance.

Runtime regressions verify IPv4/IPv6 target preservation through resolved plans
and saved profiles, malformed workflow-section validation, and readable run
listings beside damaged legacy indexes. Browser fixtures defer command responses
to check that older profile and report requests cannot replace a newer selection,
including late failures. Profile mutation tests also cover captured write targets
and deletion confirmation invalidation.

`.github/workflows/rust.yml` configures Ubuntu 22.04, Windows, macOS Apple
Silicon, and macOS Intel jobs. The macOS deployment target is 13.0; builds use
supported newer CI hosts. Local macOS checks do not verify Windows native
providers, Windows recovery, Linux authorization, or remote CI status.

See [the Rust guide](RUST.md) for pure preview commands.
Interoperability tests are limited to bounded loopback
traffic with development reference servers. Production engines do not invoke
iperf3, MTR, PowerShell, Bash, or command-line probes.

The Rust CI workflow also configures Linux loopback interoperability jobs for
iperf3 3.7 and 3.21, built from checksum-verified official source archives. These
are development references; the application does not invoke iperf3. Configuring
the jobs does not establish that remote CI passed.
