# Contributing

## Branch strategy

- `dev` — active integration branch. All feature PRs and fixes target `dev`. CI runs on every push and PR.
- `main` — release ref. Merged from `dev` via PR only. CI gates `main` identically to `dev`.

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

## Docs-first updates

Update these when behavior changes:

- `README.md`
- `docs/architecture.md`
- relevant workflow page under `docs/workflows/`
- `docs/evidence/tuning-matrix.md` for Windows tuning changes
