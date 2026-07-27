## Summary

-

## Testing

- [ ] Ran the complete gate: `./scripts/ci-local.sh`
- [ ] If the complete gate was unavailable, ran the PowerShell-only fallback: `pwsh -NoProfile -NonInteractive -File ./scripts/ci.ps1 -NoInstall`

## Risk / Impact

- [ ] Changes modify system settings
- [ ] Changes are limited to docs/CI/templates

## Checklist

- [ ] README updated if behavior or commands changed
- [ ] No secrets or sensitive data included
