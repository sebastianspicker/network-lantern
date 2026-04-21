# Diagnose Path

## Fast check

```powershell
pwsh -File .\apps\path\NetPathSuite.ps1 -DryRun -Protocols IPv4 -Rounds Standard
```

## Full path run

```bash
./apps/path/mtr-test-suite.sh --rounds Standard,TTL64 --types ICMP4,TCP4
```

## Bash defaults

The Bash entrypoint defaults to `ICMP4,ICMP6,TCP4,TCP6` × `Standard` to keep unattended runs
bounded. Use explicit `--types` and `--rounds` to extend the matrix. Empty values are rejected
as input errors.

## Result fields

Each result row includes per-stage status fields (`PingStatus`, `TracertStatus`,
`PathpingStatus`, `Tcp443Status`, `PortsStatus`) and an `OverallStatus` (`OK`/`Fail`).
`PathpingStatus` is `Skipped` (not `OK`) when `-SkipPathping` is used.

The process exits 1 if any result has `OverallStatus = Fail`.

## Questions this answers

- is the host reachable?
- where do latency and loss increase?
- does TCP succeed where ICMP fails?
- is the path stable across rounds?

