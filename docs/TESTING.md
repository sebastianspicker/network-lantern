# Testing and verification

Run every command from the repository root. Routine verification uses controlled
fakes and preview paths; it must not contact live targets or change Windows
settings.

## Toolchain

| Dependency | Version | Used by |
| --- | --- | --- |
| PowerShell | 7+ | Modules and scripts |
| PSScriptAnalyzer | 1.24.0 | PowerShell static analysis |
| Bash | 4+ for the product path CLI | Bash scripts |
| ShellCheck | Not pinned | Bash static analysis |
| Git | Not pinned | Prerequisite reporting and contributor checks |
| Node.js | 22+ | Desktop checks; development only |
| Rust | 1.96.0 | Pinned Cargo workspace and native desktop builds |

`iperf3`, `mtr`, and `column` are operator dependencies, not requirements for the
automated gate.

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

`scripts/ci-legacy.sh` first checks that `pwsh` and ShellCheck are available,
then runs, in order:

1. `make lint`: ShellCheck (`-x`) over every tracked shell script. The
   `SHELL_SCRIPTS` list in the `Makefile` is the only ShellCheck file list.
2. `scripts/Invoke-SecretScan.ps1`.
3. `scripts/ci.ps1 -NoInstall`.

`scripts/ci.ps1` runs PSScriptAnalyzer and fails if analysis reports an issue.

`scripts/ci-rust.sh` runs the desktop and Rust checks described under
[Rust migration gate](#rust-migration-gate) and the whitespace check.

If the complete gate cannot run (for example, `pwsh` is unavailable), run the
checks that can (`make lint`, `scripts/ci-rust.sh`), and name
every skipped check and the resulting gap when reporting. A PowerShell-only run
of `scripts/ci.ps1 -NoInstall` is not equivalent to the gate.

By default, the gate installs no operating-system packages and no PowerShell
modules. Where current-user PowerShell module installation is allowed, run:

```bash
NETWORK_LANTERN_INSTALL_MISSING_MODULES=1 ./scripts/ci-local.sh
```

That option installs only the pinned PSScriptAnalyzer version.

## PowerShell and focused checks

Run the PowerShell phase alone without installing dependencies:

```powershell
pwsh -NoProfile -NonInteractive -File .\scripts\ci.ps1 -NoInstall
```

Omit `-NoInstall` to install missing pinned PowerShell modules for the current
user. When needed, the script changes the PowerShell Gallery trust policy
temporarily and restores the previous value afterward.

## Make targets

| Target | Scope |
| --- | --- |
| `make lint` | ShellCheck only |
| `make test-pwsh` | `scripts/ci.ps1 -NoInstall` |
| `make test` | The PowerShell gate; omits ShellCheck and the secret scan |
| `make ci-local` | Complete cross-shell gate |

The legacy suite has no separate build or package step. The Rust and desktop
checks below add formatting, type checking, linting, and builds. There is
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

- `path-lint-test` (displayed as "Path lint and tests") runs
  `scripts/ci-legacy.sh`, the legacy half of the gate, on Ubuntu. The job keeps
  its historical name because required status checks may refer to it.
- `powershell-lint-test` runs the secret scan and PowerShell gate on Windows.

The workflow pins third-party actions by commit and caches PSScriptAnalyzer
1.24.0.

`.github/workflows/rust.yml` runs the steps of `scripts/ci-rust.sh` on Ubuntu,
Windows, and both macOS architectures.
`.github/workflows/pages.yml` publishes `site/` to GitHub Pages. Neither workflow's configuration proves that remote CI has passed.

## What the checks cover

The automated gate runs ShellCheck, PSScriptAnalyzer, the secret scan, Rust
formatting, Clippy, and the Rust unit tests, and the desktop type check and
production build.

## What the checks do not cover

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
- Preserve LF line endings. Only the directly invoked shell scripts are Git mode
  `100755`.

## Rust migration gate

`scripts/ci-rust.sh` is the second half of `scripts/ci-local.sh`. It requires Rust 1.96.0 and the desktop Node dependencies installed
with `npm ci` under `desktop/`. It runs:

- Rust formatting and Clippy with warnings denied;
- workspace unit and integration tests;
- a CLI release build;
- desktop TypeScript checks and the production frontend build;
- a production Tauri host and helper build.

The desktop type check covers the interface sources.

`.github/workflows/rust.yml` configures Ubuntu 22.04, Windows, macOS Apple
Silicon, and macOS Intel jobs. The macOS deployment target is 13.0; builds use
supported newer CI hosts. Local macOS checks do not verify Windows native
providers, Windows recovery, Linux authorization, or remote CI status.

See [the Rust guide](RUST.md) for pure preview commands.
Production engines do not invoke iperf3, MTR, PowerShell, Bash, or command-line probes.
