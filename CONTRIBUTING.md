# Contributing

Changes land through pull requests against `main`. Network Lantern is an alpha,
but compatibility changes still need to be deliberate, tested, and documented.

## Set up

Keep the source checkout layout intact. The toolchain and its pinned versions are
listed in [docs/TESTING.md](docs/TESTING.md#toolchain).

Check what is already available, without installing anything:

```powershell
pwsh -NoProfile -NonInteractive -File .\scripts\Test-Prerequisites.ps1
```

Run the complete gate from Bash, Git Bash, or WSL:

```bash
./scripts/ci-local.sh
```

The gate does not install operating-system packages. See
[docs/TESTING.md](docs/TESTING.md) for dependency installation, focused checks,
Make targets, CI coverage, and known gaps.

## Change workflow

1. Branch from current `main`.
2. Make one related set of changes.
3. Add or update tests for observable behavior.
4. Update the owning operator or maintainer documentation.
5. Run focused checks while you iterate.
6. Run `./scripts/ci-local.sh` and `git diff --check` before opening a pull
   request.
7. Open the pull request against `main`; do not push directly to `main`.

## Code boundaries

- Keep root and `apps/` entrypoints thin. They bind parameters, import a
  capability boundary, and translate process status.
- Put PowerShell behavior in the owning module. Keep a complete explicit load
  order, define public commands under `Public/`, and never import another module's
  `Private/` files.
- Compose the Bash path package only through `src/bash/path/load.sh`. Libraries
  must not source one another.
- Keep product code independent of `scripts/`.
- Keep Windows path, MTR path, throughput, and Windows tuning behavior separate.
  Similar native command parameters are not a shared domain model.
- Keep Windows tuning optional, and preserve verified backup-before-mutation and
  fail-closed restore validation.
- Keep the workflow plan module pure. Adapter paths and allowed child parameters
  belong to the trusted descriptor table in `apps/workflow/Private/`.
- Keep `site/` static and non-persistent. It must not run diagnostics or call a
  service.

The full component and dependency model is in
[docs/architecture.md](docs/architecture.md).

## Compatibility and safety

Preserve documented entrypoint paths, public exports, CLI parameters,
configuration and profile precedence, exit codes, output schemas, and default
state locations unless changing them is the point of the pull request.

Normal path, throughput-test, and tuning previews must not probe the network or
create result or tuning state. Throughput profile save and delete are the
exception: they write even when `-WhatIf` is also supplied.

Do not run live probes, throughput loads, or Windows mutation as routine test
steps. Live testing needs an authorized target and environment. Elevated Windows
apply-and-restore testing belongs on a disposable VM with an independent recovery
method.

## Windows and shell checkouts

`.gitattributes` normalizes text files to LF. Do not convert shell or Bats files
to CRLF. Only shell scripts that are invoked directly are executable in Git; every
other tracked file is `100644`. When you add or rename such a script, record Git
mode `100755` and add it to the executable allowlist in
`tests/architecture/RepositoryArchitecture.Tests.ps1`:

```bash
git add --chmod=+x path/to/entrypoint.sh
git ls-files --stage path/to/entrypoint.sh
```

The executable scripts are `apps/path/test-network-path.sh` and
`scripts/{ci-local,ci-legacy,ci-rust,install-test-deps,prepare-macos-app,run-workflow}.sh`.

## Documentation ownership

- `README.md`: purpose, requirements, safe quick start, state locations, and
  documentation navigation.
- `docs/architecture.md`: component ownership, dependency rules, runtime flows,
  trust boundaries, and invariants.
- `docs/TESTING.md`: toolchain, complete and focused gates, CI, and verification
  gaps.
- `docs/workflows/`: exact operator behavior for path, throughput, and Windows
  tuning.
- `docs/evidence/tuning-matrix.md`: implemented and excluded tuning settings and
  their automated evidence scope.
- `docs/migration/`: guides for operators arriving from the four predecessor
  tools.
- `SECURITY.md`: vulnerability reporting and operational trust guidance.
- `CHANGELOG.md`: release-facing changes under the established Unreleased section.

Examples run from the repository root unless their text says otherwise. Verify
command names, paths, parameters, defaults, output names, and links against the
implementation.

## Before opening a pull request

```bash
./scripts/ci-local.sh
git diff --check
git status --short
pwsh -NoProfile -NonInteractive -File scripts/Invoke-SecretScan.ps1
```

The secret scan checks tracked and non-ignored untracked files and suppresses
matching content from its output. It is a backstop, not a substitute for review.

Do not include live diagnostic output, saved profiles, packet captures, registry
exports, tuning backups, credentials, or internal host information unless the
data is intentionally sanitized and required as a fixture.

In the pull request, describe the behavior you changed, the affected platforms and
entrypoints, the commands you ran, any skipped checks and why, and the remaining
operational or security limitations.
