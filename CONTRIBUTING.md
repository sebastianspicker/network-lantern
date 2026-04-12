# Contributing

## Prereqs
- PowerShell 7+

## Local checks

```powershell
pwsh -NoProfile -Command 'Install-Module PSScriptAnalyzer,Pester -Scope CurrentUser -Force'
pwsh -NoProfile -Command 'Invoke-ScriptAnalyzer -Path . -Recurse'
pwsh -NoProfile -Command 'Invoke-Pester -Path ./tests -CI'
```

## Notes
- Keep tests offline (no registry/network/NIC changes).
- Prefer shared helpers in `WindowsUdpJitterOptimization/Private` instead of duplication.
