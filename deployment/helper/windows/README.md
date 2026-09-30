# Windows privileged helper deployment

Network Lantern supports Windows 10 and Windows 11 on x64.

Package the signed `network-lantern-helper.exe` beside the signed Network Lantern
application executable in an application directory under the system Program Files
folder. Registration resolves that exact sibling file name, verifies that the
unelevated application user cannot mutate the file or its directory, verifies its
Authenticode signature without network retrieval, and invokes only the fixed
`--install-service <owner SID>` mode through the native `runas` verb.

The elevated process creates the demand-start `NetworkLanternHelper` LocalSystem
service with this fixed command line:

```text
"<absolute deployment directory>\network-lantern-helper.exe" --service
```

The application itself stays unelevated. It talks to the helper through the
local-only `\\.\pipe\NetworkLantern.Helper.v1` named pipe. The pipe ACL admits
LocalSystem, built-in administrators, and the registered user SID. Each operation
also uses the per-user HMAC credential stored below
`%ProgramData%\NetworkLanternHelper\clients`; the user gets read-only access to
its credential and traversal access to the protected parent directories. Requests
are capped at 1 MiB, connections at 64, and framing operations at five seconds.

Removal first reserves the service's operation admission slot. It refuses while
work is active, then launches the fixed `--remove-service` mode through UAC. The
elevated remover stops and deletes the SCM service before deleting the bounded
owner and credential files. No registration or removal path accepts an executable
path, registry path, shell command, or extra service argument.

Development builds normally lack an Authenticode signature, so they cannot be
registered. Sign the final helper before testing registration. Validate the
registration, cancellation, stop, and removal paths on matching Windows 10 and
Windows 11 x64 hosts before release: cross-compilation does not exercise UAC, SCM,
ACL, token impersonation, or named-pipe runtime behavior.
