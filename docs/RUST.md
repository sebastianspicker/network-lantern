# Rust application

The Rust workspace and desktop are the application under development. The
PowerShell and Bash implementation stays as the behavior reference until the
archive trigger in [architecture](architecture.md#legacy-reference-and-archive-trigger)
is met. A local
macOS build makes no installer, signing, or operational Windows recovery claim.

## Local CLI

The pinned toolchain is Rust 1.96.0. Build and inspect the CLI:

```sh
cargo build --release --locked -p network-lantern
./target/release/network-lantern --help
./target/release/network-lantern doctor --json
./target/release/network-lantern throughput --target fixture.invalid --dry-run --json
./target/release/network-lantern workflow baseline --target fixture.invalid --dry-run
```

When launched from a legacy checkout, the CLI and desktop load `config/hosts.conf`
before applying named profiles and explicit settings. The resolved host layer is
snapshotted for the reviewed run, so later file changes do not change its targets.
A missing or empty file keeps the built-in fallback hosts.

A preview performs no DNS lookup, connection, helper authorization, or result
write. Profile save and delete, and helper registration, are separate explicit
operations. Execution sends real traffic, so use only authorized targets.

Throughput defaults include simultaneous TCP transmit and receive; set
`--bidirectional false` explicitly for servers without that feature. The full
matrix plans 1,145 measurements with bidirectional TCP and 1,100 without, and
`--single-test` selects one transmit measurement. `--max-total-tests 0` is
unlimited, and retries do not consume the planned budget. A budget violation is
rejected before networking or output creation, and nominal duration is the planned
count multiplied by duration plus omission.

Configuration precedence is defaults, named profile, JSON configuration, then
explicit settings and CLI flags. `--settings` accepts a JSON object; unknown keys
produce warnings, and `--strict` rejects them. Repeated matrix entries are
preserved. Existing throughput profile names and PowerShell parameter keys are
read from `.iperf3/profiles.json`, and the originals are not rewritten during
planning.

Resolved native path settings preserve empty `hosts_ipv4` and `hosts_ipv6`
lists as exclusions. Saving and reloading a single-target profile keeps the same
targets and measurement count. Empty legacy `HostsIPv4`/`HostsIPv6` aliases retain
their fallback behavior; use the native keys when explicitly excluding a family.

```sh
./target/release/network-lantern profiles list
./target/release/network-lantern profiles show office
./target/release/network-lantern throughput --profile office --config config.json --dry-run
./target/release/network-lantern path basic --target fixture.invalid --settings '{"rounds":["Standard"],"skip_pathping":true}' --dry-run
./target/release/network-lantern path trace --target fixture.invalid --settings '{"rounds":["Standard"],"types":["ICMP4"],"cycles":3}' --dry-run
```

Path engines keep separate round and result semantics. Native socket access may
require platform authorization, and AS4/AS6 enrichment sends hop addresses to Team
Cymru DNS only when it is selected. Diagnostic text is native output, not an
imitation of Windows commands or a byte-identical MTR copy.

Sampling cadence works like this:

- Basic echo samples keep a one-second cadence.
- Repeated hop sampling rotates through hops every 250 ms.
- MTR-style traces pace individual TTL probes by dividing the cycle interval by
  the discovered hop count, starting from an estimate of ten hops.
- The pinned MTR 0.96 reference uses a ten-second probe timeout and ends a sweep
  after at most 12 unknown prior hops.

Packet sizes include IP and transport headers; the MTU1400 trace round does not
force IPv4 DF.

Trace statistics accumulate incrementally. Each hop retains at most 64 distinct
responders and MPLS stacks, and warns explicitly on a partial result if that limit
is exceeded. A trace stops with a partial outcome once its 512 outstanding native
probes limit is reached, and delayed scheduling does not send catch-up bursts.

After the final sweep, the response window ends five seconds after the next
cadence tick, matching MTR reporting. Sent probes still unresolved at that cutoff
count as timeout loss. User cancellation keeps a separate cancelled-sample count
and does not turn packets with an interrupted response window into measured loss.
Work cancelled before a packet is sent does not increase sent counts.

## Saved data

Each Rust execution writes a UUID-named directory under `logs` by default.
Measurements are atomically published separately, and the summary records planned,
executed, failed, succeeded, and skipped counts. Records carry a schema version
and engine provenance. Cancellation preserves completed measurements and a partial
summary when the filesystem remains writable.

```sh
./target/release/network-lantern runs list logs --limit 50
./target/release/network-lantern runs show logs/RUN-ID/summary.json
./target/release/network-lantern runs show legacy-path.csv --offset 0 --limit 50
./target/release/network-lantern runs compare baseline.json current.json
./target/release/network-lantern runs export baseline.json exported.json
```

Readers accept legacy summaries, run indexes, JSON and CSV path and measurement
reports, and native diagnostic text. Standalone JSON exports include persisted
native measurements and identify missing artifacts. The JSON reader and export
limit is 16 MiB; keep the complete run directory for larger runs.

CSV pages scan records without retaining the complete row set. Each page holds at
most 100 rows and 1 MiB of serialized data; a row is limited to 256 KiB, and
headers to 256 unique columns. Full CSV reads reject files over any of these
limits and point the operator to pagination:

- more than 100,000 rows,
- more than one million cells,
- more than 16 MiB of represented data.

Unknown Rust record versions fail explicitly. Run-directory listings retain at
most 100,000 paths and 16 MiB of path text, and return at most 1 MiB per page.
Oversized summary metadata or legacy indexes are reported explicitly with a path
so an operator can inspect them directly.

A corrupt or unreadable legacy run index is returned as a path-qualified error
alongside the run list. It does not prevent browsing other readable summaries.

Unavailable metrics remain absent. Threshold breaches have their own count and do
not turn successful measurements into transport failures. Profile writers hold
stable sidecar locks across atomic replacement and refuse invalid stores rather
than overwriting unreadable data. File reads are bounded; unprivileged path
revalidation is not a replacement for the protected Windows recovery boundary.

## Process status

Throughput uses `0` for success, `11` for invalid input, `12` for missing
prerequisites, `13` for connectivity, `14` for partial failure or threshold
breach, `15` for total failure, and `16` for internal failure. Other CLI commands
return `1` on failure, and SIGINT and SIGTERM map to `130` and `143` on Unix.
Machine-readable errors contain a category and a bounded message. Measurements run
sequentially, and a second run is rejected while the shared runtime is active.

## Verification

`./scripts/ci-local.sh` includes the legacy gate and `scripts/ci-rust.sh`. The Rust
phase checks formatting, Clippy, workspace tests, CLI and desktop builds,
TypeScript, and frontend tests. The separate Rust CI workflow configures Windows,
Ubuntu 22.04, and both macOS architectures. Its configured jobs are not proof that
remote CI has passed.

## Desktop and authorization

```sh
npm --prefix desktop ci
npm --prefix desktop run build
cargo build --locked -p network-lantern-desktop -p lantern-helper
./target/debug/network-lantern-desktop
```

Triage, Path, Throughput, Baseline, Windows tuning, Profiles, and Reports use typed
Rust operations. A changed configuration invalidates its preview, cancellation
stays accessible during a run, and report pages and recent activity are bounded.
Helper registration and removal are explicit actions, serialized against active
measurements.

Profiles and Reports keep asynchronous results attached to the selection that
requested them. Selecting another profile or report prevents an older response
from replacing the current evidence or editor contents. Profile writes use the
name and store captured when Save or Confirm deletion was selected; another
write is unavailable until that operation finishes. Editing or selecting a
profile clears its pending deletion confirmation. Report exports likewise finish
before another export can start, including when the selected report changes.

The runtime uses an authenticated registered helper for native path and preflight
operations. Windows DSCP measurements use native qWAVE and route through the
authenticated helper when it is registered, while other throughput data sockets
stay in the unprivileged runtime. The reviewed helper request includes the concrete
measurement and bounded client deadlines.

On macOS, the prepared app bundle must have Developer ID signatures and system
approval before privileged XPC is available. See the
[deployment requirements](../deployment/helper/macos/README.md). An unsigned local
desktop build can plan, inspect records, and perform unprivileged measurements; it
reports privileged helper unavailability explicitly. Linux uses systemd and polkit
with a protected Unix socket. Windows uses UAC and a protected named pipe.
Deployment assets and cross-compilation do not establish operational acceptance on
matching hosts.

Throughput preflight checks ICMP reachability, the TCP server port, and optional
MTU probes. For an intentionally TCP-only environment, set
`{"skip_reachability_check":true,"disable_mtu_probe":true}` in settings.

UDP sender loss is reported over the peer's whole run when its iperf result has no
separate omission baseline; receiver loss excludes omission. Each result records
its loss scope, and unavailable OS counters stay unavailable.
