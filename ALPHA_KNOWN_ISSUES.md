# Known issues in this alpha

Thank you for testing Network Lantern `0.1.0-alpha.1`. This page lists what does
not work yet, what has not been checked, and how to report a problem.

## What to test

The alpha is the Rust command-line tool on macOS. Build it and try it with the
steps in the README section "Try the Rust CLI". Linux and Windows builds compile,
but nobody has run them yet, so treat them as extra. The desktop app, the
PowerShell and Bash tools, and Windows tuning are not part of this alpha.

Probe only hosts and iperf3 servers you are allowed to test. Every command
without `--dry-run` sends real traffic.

## Not checked yet

- **Traces across real networks.** Path traces without root were checked only
  against this computer itself. We have not yet confirmed that every router
  along a real route shows up for UDP and TCP traces. If hops are missing that
  `traceroute` shows, please report it.
- **Running with `sudo`.** The tool is designed to work without root on macOS.
  Running it with `sudo` has not been tested. If you do, give the output folder
  as a full path (for example `--out "$PWD/logs"`). The results then belong to
  root, so read them with `sudo` too.
- **Linux and Windows.** They compile, but nobody has run them yet.

## Known problems

- **The last hop of a trace appears twice.** A trace usually lists the
  destination once more, one hop further, with a single probe. The timings of
  the real destination hop are correct. Ignore the extra row.
- **Throughput needs a normal iperf3 server.** Before measuring, the tool checks
  that the server's port is open by connecting and disconnecting once. Start the
  server with `iperf3 -s`. A one-shot server (`iperf3 -s -1`) counts that check
  as its only test, exits, and the measurement then fails with "Connection
  refused". The server also prints `unable to receive cookie at server` once per
  run. That message is harmless.
- **TCP traces reach the destination.** On macOS, a TCP trace opens a real
  connection to the destination and closes it immediately. Firewalls or logs on
  the destination may record these connections.
- **The background helper is not available.** Source builds cannot register
  the privileged helper, because it needs a signed app. You do not need it on
  macOS. If `doctor` says ICMP is unavailable, see "Native socket access" in the
  Rust guide.
- **The build may use the wrong Rust version.** If `cargo --version` does not
  print 1.96.0 (Homebrew's cargo, for example), the build may fail or warn.
  Follow the README note to use rustup's toolchain.
- **Formats can change.** Command options, profiles, result files and folder
  layout may still change before a stable release.

## Reporting a bug

Open an issue in the GitHub repository with the bug report template, and
include:

1. The exact command you ran and its exit code.
2. The output of `network-lantern --version` and `network-lantern doctor --json`.
3. The same command run again with `-v`, plus the run folder it printed: the
   files `run.log` and `summary.json`, and the `measurement-*.json` files if the
   problem is in the results.
4. Your macOS version and whether you ran the command with `sudo`.

These files contain the addresses of your network and the hosts you tested.
Read them first, and remove or replace anything you do not want to make public.
Report security problems privately, as described in the security policy
(`SECURITY.md`), and not in a public issue.
