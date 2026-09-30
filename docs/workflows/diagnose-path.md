# Path diagnostics

Network Lantern ships two path tools. Pick the one that matches your platform:

- **PowerShell** for the Windows built-in tools (`ping`, `tracert`, `pathping`).
- **Bash** for an `mtr` matrix on macOS and Linux.

The two tools are deliberately independent. They share `config/hosts.conf`, but
their rounds, protocols, and result files differ.

## PowerShell entrypoint

List the supported rounds and protocols:

```powershell
pwsh -NoProfile -File .\apps\path\Test-NetworkPath.ps1 -ListRounds
pwsh -NoProfile -File .\apps\path\Test-NetworkPath.ps1 -ListProtocols
```

Rounds: `Standard`, `MTU1400_DF`, `TTL64_Timeout5s`. Protocols: `IPv4`, `IPv6`.

Preview an IPv4 run:

```powershell
pwsh -NoProfile -File .\apps\path\Test-NetworkPath.ps1 `
  -HostsIPv4 example.com -Protocols IPv4 -Rounds Standard -DryRun
```

Run the same selection and skip `pathping`:

```powershell
pwsh -NoProfile -File .\apps\path\Test-NetworkPath.ps1 `
  -HostsIPv4 example.com -Protocols IPv4 -Rounds Standard -SkipPathping
```

A live run needs Windows. For every selected round, protocol, and host, the
script runs `ping`, `tracert`, optional `pathping`, and a TCP 443 check through
`Test-NetConnection`.

Output goes to `~/logs` by default; use `-LogDirectory` to change it. Each run
writes two files:

- `net_results_<timestamp>_<pid>.json`
- `net_summary_<timestamp>_<pid>.csv`

Each result record carries `PingStatus`, `TracertStatus`, `PathpingStatus`,
`Tcp443Status`, `PortsStatus`, and `OverallStatus`. With `-SkipPathping`,
`PathpingStatus` is `Skipped`. `Ports` is always an empty array and `PortsStatus`
is always `Skipped`; both fields are retained for existing consumers.

The process exits 1 when any planned run has `OverallStatus` equal to `Fail`.
Validation and execution errors also return a nonzero status. If execution is
interrupted after at least one result is collected, the script tries to write the
partial results.

Each JSON or CSV file is published through an exclusive temporary file beside
its destination. The file is flushed and closed before an atomic replacement, so
a failed write leaves the previous content intact. Atomicity is per file; the
JSON and CSV pair is not a transaction.

## Bash entrypoint

List the supported test types and rounds:

```bash
./apps/path/test-network-path.sh --list-types
./apps/path/test-network-path.sh --list-rounds
```

Test types: `ICMP4`, `ICMP6`, `UDP4`, `UDP6`, `TCP4`, `TCP6`, `MPLS4`, `MPLS6`,
`AS4`, `AS6`. Rounds: `Standard`, `MTU1400`, `TOS_CS5`, `TOS_AF11`, `TTL10`,
`TTL64`, `FirstTTL3`, `Timeout5`.

Preview a matrix:

```bash
./apps/path/test-network-path.sh \
  --hosts4 example.com --types ICMP4,TCP4 \
  --rounds Standard,TTL64 --dry-run
```

Run it:

```bash
./apps/path/test-network-path.sh \
  --hosts4 example.com --types ICMP4,TCP4 \
  --rounds Standard,TTL64
```

A live run needs Bash 4+, `mtr`, and `jq`. The text summary also needs `column`;
pass `--no-summary` when it is missing. Each `mtr` command requests 300 report
cycles, and commands run one at a time with a 360-second timeout. Set
`MTR_TIMEOUT_SECONDS` to a different positive integer to change the timeout.

While a command runs, the tool polls with 0.5-second sleeps, so completion and
signal response can take up to about 0.5 seconds plus scheduling delay. Timeout
escalation keeps 0.1-second TERM grace checks before KILL.

The default matrix is `ICMP4,ICMP6,TCP4,TCP6` by `Standard`, and TCP modes use
port 443. Pass explicit `--types`, `--rounds`, and host arguments to keep a live
run bounded.

Output goes to `LOG_DIR`, or `~/logs` by default. The command writes:

- `mtr_results_<timestamp>_<pid>.json.log`
- `mtr_summary_<timestamp>_<pid>.log`

The JSON log is a stream of consecutive JSON objects, one per planned run, not a
JSON array. A failed command or an invalid native result becomes a parseable
failure object. The process exits 1 when any planned run fails.

## Rust CLI and desktop

The Rust migration's CLI and desktop have separate native engines; see the
[Rust guide](../RUST.md). Their resolved settings and saved profiles preserve a
single target's address family across preview and reload. In native JSON,
`hosts_ipv4: []` or `hosts_ipv6: []` explicitly excludes that family's targets.
Legacy camel-case host aliases retain their fallback behavior.

## Default targets

Both entrypoints read `config/hosts.conf` unless you supply hosts explicitly.
The checked-in file currently contains:

- IPv4: `cloudflare.com`, `google.com`, `wikipedia.org`, `amazon.com`
- IPv6: `cloudflare.com`, `google.com`, `wikipedia.org`

These are third-party services. Review or replace them before collecting live
diagnostics.

## Troubleshooting

- Use Bash with a working `mtr` installation for live non-Windows diagnostics.
- Pass `--no-summary` if `column` is missing.
- A no-work plan means the type, protocol, or host selection is incompatible.
- Use `-Quiet` or `--quiet` to suppress progress while keeping warnings, failures,
  and the final summary.
