# Windows Tuning

Windows tuning is optional. Use it to test whether endpoint-side policy changes improve consistency. It is not required for path or throughput diagnostics.

## Backup only

```powershell
pwsh -File .\apps\windows-tuning\Optimize-NetworkPath.ps1 -Action Backup
```

## Dry-run safe profile

```powershell
pwsh -File .\apps\windows-tuning\Optimize-NetworkPath.ps1 -Action Apply -TuningProfile Safe -UdpPorts 5201 -DryRun
```

## Verify current state

```powershell
pwsh -File .\apps\windows-tuning\Optimize-NetworkPath.ps1 -Action Verify -UdpPorts 5201 -PassThru
```

`Verify` requires Windows and the `NetQosPolicy` cmdlets. On non-Windows platforms or when QoS
enumeration is unavailable, `Success` is `False` and affected components report `Unknown`.

## Restore

```powershell
pwsh -File .\apps\windows-tuning\Optimize-NetworkPath.ps1 -Action Restore
```

Restore validates the backup manifest before making any changes. Incompatible manifests
(wrong schema version or tool name) are rejected with a `Warn` result; no destructive work
is attempted on an incompatible backup.

