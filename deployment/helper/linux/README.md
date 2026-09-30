# Linux helper deployment

Install the release-built `network-lantern-helper` as the root-owned, non-writable
file `/usr/libexec/network-lantern-helper`. Install the template unit in
`/usr/lib/systemd/system/` and the JavaScript rule in
`/usr/share/polkit-1/rules.d/`, then run `systemctl daemon-reload`.

The desktop and CLI request `network-lantern-helper@<uid>.service` through the
systemd D-Bus API with interactive polkit authorization. The rule permits only an
active local administrator to start or stop a numerically scoped Network Lantern
unit. It grants no general service-management permission.

The root service creates one mode-0600 Unix socket for the requested UID under
`/run/network-lantern`, plus a mode-0600, caller-owned 32-byte credential under
`/var/lib/network-lantern/clients`. Each actual service start holds its per-UID
instance lock before atomically replacing any credential from an older
incarnation.

Normal and failed shutdowns revoke the credential. As a process-failure fallback,
the unit's `ExecStopPost` invokes the helper's fixed cleanup mode. Cleanup takes
no path or UID argument; it validates the canonical service UID environment and
protected parent directories, and acquires the per-UID instance lock before
removing the exact credential. A denied stop or a failed duplicate start
therefore leaves the running service and its credential intact.

The Linux authorization boundary is the approved Unix account: processes under
that UID can read its credential and use its socket. It does not assert a
code-signing identity for the calling application.

Before dispatch, the service validates root-owned ancestors, peer credentials,
request bounds, reviewed-operation hashes, timestamps, and replay nonces. One
operation runs at a time; removal reserves the operation slot and fails while a
mutation or recovery operation is active. Client disconnect and service shutdown
cancel in-flight diagnostics.

These files are deployment inputs only. Repository tests do not install the unit,
invoke polkit, elevate, or run live probes. Operational acceptance needs a
matching-host package install plus tests for authorization denial, wrong UID,
framing limits, disconnect cancellation, service shutdown, and safe removal.
