# Choosing a workflow

| Need | Command or workflow | Notes |
| --- | --- | --- |
| Reachability, route, latency, or loss | direct path entrypoint or `-Workflow Path` | PowerShell live runs require Windows; Bash live runs require `mtr` |
| Path evidence, with optional throughput | `-Workflow Triage` | Throughput runs only when `-IperfTarget` is set |
| TCP or UDP load measurement | direct throughput entrypoint or `-Workflow Throughput` | Requires an `iperf3` server |
| Path plus one throughput sample | `-Workflow Baseline` | Requires `-IperfTarget` |
| Inspect expected Windows QoS state | `-Workflow WindowsTuning -TuningAction Verify` | Read-only |
| Preview Windows changes | `-Workflow WindowsTuning -TuningAction Apply -DryRun` | Does not require elevation |
| Apply or restore Windows changes | direct tuning entrypoint or `WindowsTuning` workflow | Requires elevation and an independent recovery plan |

Start by previewing the selected command. For example:

```powershell
pwsh -NoProfile -File .\Invoke-NetworkLantern.ps1 `
  -Workflow Triage -IperfTarget iperf3.example.net -DryRun
```

For a live investigation:

1. Collect path evidence with explicit hosts and protocols.
2. Run throughput only against a trusted or operator-controlled `iperf3`
   server.
3. Keep the input parameters and output files for both runs.
4. If Windows endpoint policy remains relevant, run tuning `Verify` and an
   Apply dry run.
5. Apply changes only when the system has a tested recovery path. Repeat the
   same diagnostics afterward so results are comparable.

The orchestrator stops when a child exits nonzero and returns that child's exit
code.
