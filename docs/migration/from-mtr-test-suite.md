# Migrating from mtr-test-suite

The Bash path command now lives at `apps/path/test-network-path.sh`. The repository
also ships a separate Windows PowerShell path implementation at
`apps/path/Test-NetworkPath.ps1`.

| Previous surface | Current surface |
| --- | --- |
| `mtr-test-suite.sh` | `apps/path/test-network-path.sh` |
| no PowerShell equivalent | `apps/path/Test-NetworkPath.ps1` |
| standalone use only | direct entrypoint or `Invoke-NetworkLantern.ps1 -Workflow Path` |

`config/hosts.conf` is still the shared default target file, and the Bash default
matrix is `ICMP4,ICMP6,TCP4,TCP6` by `Standard`. Select any other types and rounds
explicitly.

Preview the current Bash plan:

```bash
./apps/path/test-network-path.sh --dry-run
```

Live runs need Bash 4+, `mtr`, and `jq`. The text summary also needs `column`; use
`--no-summary` without it. Each `mtr` process has a 360-second default timeout,
controlled by `MTR_TIMEOUT_SECONDS`.

The JSON log is a stream of JSON objects, not one JSON array. Parse each value in
the stream separately.
