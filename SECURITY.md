# Security

## Reporting a vulnerability

Report suspected vulnerabilities privately to the repository owner. Do not
open a public issue with exploit details, credentials, internal infrastructure,
or unsanitized diagnostic output.

## Sensitive operational data

Treat the following as private unless deliberately sanitized:

- hostnames, IP addresses, routes, DNS results, and packet captures
- throughput targets, raw `iperf3` output, run indexes, and saved profiles
- registry exports, QoS policy exports, NIC details, and tuning backups
- local paths, usernames, machine identifiers, logs, and crash output

The supported local storage and output paths are ignored by Git. Run the
repository secret scan and inspect `git status --short` before publishing.

## Security boundaries

- The suite exposes no network service or authentication surface.
- Diagnostic targets are operator-supplied and passed to native tools through
  validated argument arrays.
- Windows tuning is optional. Apply and restore operations can require
  administrator rights and modify registry, QoS, NIC, and power-plan state.
- Restore validates manifest compatibility and artifact integrity before
  applying backup contents.
- The latest `main` branch is the supported source version.
