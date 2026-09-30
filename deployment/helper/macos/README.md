# macOS helper deployment

The helper uses the public SMAppService LaunchDaemon interface and authenticated
XPC on macOS 13 or later. Both peers require an Apple-trusted Developer ID
signature from the same team. The app signing identifier is
`dev.network-lantern.desktop`, the CLI uses `dev.network-lantern.cli`, and the
helper identifier and Mach service are `dev.network-lantern.helper`. Code identity
comes from the running signed image and message audit tokens; process IDs are
diagnostic information, not proof of identity.

Build the production frontend, desktop, CLI, and helper, then run
`scripts/prepare-macos-app.sh release`. This prepares a new local app bundle and
refuses an existing destination. It performs no signing, installation,
registration, or privileged operation. The daemon plist belongs in
`Contents/Library/LaunchDaemons`, and its `BundleProgram` points at
`Contents/Library/HelperTools/network-lantern-helper`.

Distribution must sign the helper with its helper identifier, the CLI with its CLI
identifier, and the app with its app identifier, all using the same Developer ID
team. Sign nested executables before the enclosing bundle. Release signing and
notarization are separate from the local build. An unsigned or ad-hoc build
reports helper unavailability and cannot satisfy this authorization boundary.

The application's explicit Register helper action calls SMAppService. macOS may
require approval in System Settings before the daemon can run; register again
after approval to finish credential provisioning. The kernel authenticates the XPC
connection before the service creates a caller-owned, mode-0600 key under
root-owned `/Library/Application Support/NetworkLantern/clients`. Credentials are
never accepted from request paths. Every operation carries a reviewed-operation
hash, run ID, expiring authenticated frame, and replay nonce.

Remove helper refuses an active operation and reserves a short removal window
before calling SMAppService unregister. It does not delete saved diagnostics or
recovery data. A client disconnect cancels its diagnostic work. Status requires an
authenticated XPC response before it reports the service reachable.

Operational acceptance requires a properly signed bundle, system approval, and
matching-host tests of registration, peer rejection, disconnection, cancellation,
and removal. A successful unsigned local build does not prove these checks.

References: [SMAppService daemon registration](https://developer.apple.com/documentation/servicemanagement/smappservice/daemon(plistname:)),
[XPC peer code-signing requirements](https://developer.apple.com/documentation/xpc/xpc_connection_set_peer_code_signing_requirement(_:_:)).
