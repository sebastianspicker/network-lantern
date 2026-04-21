# Diagnose Throughput

## Single baseline

```powershell
pwsh -File .\apps\throughput\iPerf3Test.ps1 -Target iperf3.example.net -SingleTest
```

## Matrix run

```powershell
pwsh -File .\apps\throughput\iPerf3Test.ps1 -Target iperf3.example.net -Protocol Both -Summary
```

## Questions this answers

- what throughput do I get on TCP and UDP?
- where do loss and jitter begin under UDP load?
- how does the current run compare to baseline artifacts?
