# Migrating from mtr-test-suite

The Bash path command moved to `apps/path/test-network-path.sh`. The repository
also contains a separate Windows PowerShell path implementation at
`apps/path/Test-NetworkPath.ps1`.

| Previous surface | Current surface |
| --- | --- |
| `mtr-test-suite.sh` | `apps/path/test-network-path.sh` |
| no PowerShell equivalent | `apps/path/Test-NetworkPath.ps1` |
| standalone use only | direct entrypoint or `Invoke-NetworkLantern.ps1 -Workflow Path` |

`config/hosts.conf` remains the shared default target file. The Bash default
matrix is `ICMP4,ICMP6,TCP4,TCP6` by `Standard`. Additional types and rounds
must be selected explicitly.

Preview the current Bash plan with:

```bash
./apps/path/test-network-path.sh --dry-run
```

Live runs require Bash 4+, `mtr`, and `jq`. The text summary also requires
`column`; use `--no-summary` without it. Each `mtr` process has a default
360-second timeout controlled by `MTR_TIMEOUT_SECONDS`.

The current JSON log is a sequence of JSON objects rather than one JSON array.
Consumers must parse each value in the stream.
