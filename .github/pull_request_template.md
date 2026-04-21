## Summary
- 

## Testing
- [ ] `pwsh -NoProfile -Command 'Invoke-ScriptAnalyzer -Path . -Recurse'`
- [ ] `pwsh -NoProfile -Command 'Invoke-Pester -Path ./tests -CI'`

## Risk / Impact
- [ ] Changes modify system settings
- [ ] Changes are limited to docs/CI/templates

## Checklist
- [ ] README updated if behavior or commands changed
- [ ] No secrets or sensitive data included
