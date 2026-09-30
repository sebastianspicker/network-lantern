#!/usr/bin/env python3
"""Sanitized planning/report benchmarks: one warm-up, ten measured repetitions."""
import importlib.util
import json
import pathlib
import statistics
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("preview", ROOT / "scripts/benchmark-rust.py")
preview = importlib.util.module_from_spec(spec)
spec.loader.exec_module(preview)
ARTIFACTS = ROOT / "artifacts/rust-migration/benchmark-inputs"
RUST = ROOT / "target/release/network-lantern"
ALLOC = ROOT / "target/release/examples/resource_measurement"


def summarize(samples):
    return {key: {"median": statistics.median(s[key] for s in samples),
                  "range": [min(s[key] for s in samples), max(s[key] for s in samples)]}
            for key in samples[0]}


def main():
    ARTIFACTS.mkdir(parents=True, exist_ok=True)
    reference = ROOT / "artifacts/rust-migration/legacy-reference"
    module = reference / "src/powershell/throughput/NetworkLantern.Throughput.psm1"
    wrapper = ARTIFACTS / "compare.ps1"
    # Paths are arguments; no fixture data is interpreted as PowerShell code.
    wrapper.write_text("param([string]$ModulePath,[string]$ReportPath)\nImport-Module $ModulePath -Force\nCompare-Iperf3Runs -BaselinePath $ReportPath -CurrentPath $ReportPath | ConvertTo-Json -Depth 8\n")
    results = {}
    for count in [100, 10000]:
        report = ARTIFACTS / f"summary-{count}.json"
        report.write_text(json.dumps({"SummaryVersion": 2, "Timestamp": "fixture", "Status": "Success",
                                     "Counts": {"Total": count, "Failed": 0}, "ElapsedSeconds": 10,
                                     "Results": [{"No": i, "Metrics": {"TxMbps": 42.0}} for i in range(count)]}))
        variants = {
            "legacy": ["pwsh", "-NoProfile", "-NonInteractive", "-File", str(wrapper), str(module), str(report)],
            "rust": [str(RUST), "runs", "compare", str(report), str(report)],
        }
        samples = {key: [] for key in variants}
        for command in variants.values():
            preview.measure(command)
        for iteration in range(10):
            for name in list(variants) if iteration % 2 == 0 else reversed(variants):
                samples[name].append(preview.measure(variants[name]))
        allocations = json.loads(subprocess.check_output([str(ALLOC), "report", str(report)], cwd=ROOT))
        results[f"report_{count}_rows"] = {name: summarize(values) for name, values in samples.items()}
        results[f"report_{count}_rows"]["rust_allocations"] = summarize(allocations["samples"])
    for count in [1, 128]:
        data = json.loads(subprocess.check_output([str(ALLOC), "plan", str(count)], cwd=ROOT))
        results[f"plan_{count}_by_{count}_matrix"] = summarize(data["samples"])
    print(json.dumps({"warmups": 1, "repetitions": 10, "scope": "identical legacy summary inputs; Rust allocation instrumentation is development-only; no maximum network throughput claim", "results": results}, indent=2))


if __name__ == "__main__":
    main()
