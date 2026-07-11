# Contributing

## Branch strategy

- `main` is the protected integration and release branch.
- Create a short-lived topic branch from current `main` for each change family.
- Open pull requests against `main`; CI runs on pushes and pull requests that target it.

Do not push directly to `main`.

## Development rules

- Keep modules separated by operator task: path, throughput, windows-tuning.
- Do not introduce tuning changes that are not documented in `docs/evidence/tuning-matrix.md`.
- Keep Windows tuning reversible and optional.
- Do not make the path module depend on `iperf3`.
- Prefer shared helpers in `src/` over duplicate glue in `apps/`.

## Verification

Run before publishing:

```bash
./scripts/ci-local.sh
```

If `bats` is not installed locally, install it first or use the Ubuntu CI job as the bash reference surface.
The Bats regression suite also requires Python 3 for JSON validation.

The local gate requires `git` because the secret scan checks tracked files and
untracked files that are not ignored. Keep operator output, local profiles,
tool indexes, and internal work notes in the ignored locations documented in
the README. Check `git status --short` before publishing.

## Docs-first updates

Update these when behavior changes:

- `README.md`
- `docs/architecture.md`
- relevant workflow page under `docs/workflows/`
- `docs/evidence/tuning-matrix.md` for Windows tuning changes
