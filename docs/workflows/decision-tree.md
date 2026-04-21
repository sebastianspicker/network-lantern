# Decision Tree

```mermaid
flowchart TD
    START([Start]) --> Q1{"Can I reach it?\nWhere does the path break?"}
    Q1 -->|Yes| Q2{"How much throughput\ncan I sustain under load?"}
    Q1 -->|No / Unsure| PATH["Run Path\nNetPathSuite.ps1 / mtr-test-suite.sh"]
    Q2 -->|Yes| THRU["Run Throughput\niPerf3Test.ps1"]
    Q2 -->|No / Baseline only| PATH
    PATH --> Q3{"Windows endpoint\npolicy suspected?"}
    THRU --> Q3
    Q3 -->|Yes| WINT["Run WindowsTuning\nOptimize-NetworkPath.ps1\nthen rerun diagnostics"]
    Q3 -->|No| DONE([Done])
    WINT --> DONE
```

## Start here

1. If the question is "can I reach it and where does the path break?", use `Path`.
2. If the question is "how much throughput can I sustain under load?", use `Throughput`.
3. If Windows endpoint policy might be hurting results, use `WindowsTuning` after diagnostics, not before by default.

## Suggested flow

- first-pass issue triage: `Triage`
- route instability or packet loss: `Path`
- performance baseline or saturation test: `Throughput`
- before/after endpoint comparison on Windows: `WindowsTuning` plus rerun diagnostics
