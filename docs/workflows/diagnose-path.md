# Diagnose Path

## Fast check

```powershell
pwsh -File .\apps\path\NetPathSuite.ps1 -DryRun -Protocols IPv4 -Rounds Standard
```

## Full path run

```bash
./apps/path/mtr-test-suite.sh --rounds Standard,TTL64 --types ICMP4,TCP4
```

## Questions this answers

- is the host reachable?
- where do latency and loss increase?
- does TCP succeed where ICMP fails?
- is the path stable across rounds?
