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

## Restore

```powershell
pwsh -File .\apps\windows-tuning\Optimize-NetworkPath.ps1 -Action Restore
```
