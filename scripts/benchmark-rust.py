#!/usr/bin/env python3
"""Compare pure CLI preview startup with the preserved PowerShell implementation.

One warmup and ten measured repetitions, alternating order. No network targets
are contacted and no measurement artifacts are written. Output is JSON.
"""
import json
import pathlib
import re
import sys
import statistics
import subprocess
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
RUST = ROOT / "target/release/network-lantern"
LEGACY = [
    "pwsh", "-NoProfile", "-NonInteractive", "-File",
    str(ROOT / "artifacts/rust-migration/legacy-reference/apps/throughput/Measure-NetworkThroughput.ps1"),
    "-Target", "fixture.invalid", "-WhatIf", "-Quiet",
]
COMMANDS = {
    "full_matrix_preview": {
        "legacy": LEGACY,
        "rust": [str(RUST), "throughput", "--target", "fixture.invalid", "--dry-run", "--json"],
    },
    "single_test_preview": {
        "legacy": LEGACY + ["-SingleTest"],
        "rust": [str(RUST), "throughput", "--target", "fixture.invalid", "--single-test", "--dry-run", "--json"],
    },
}


def measure(command):
    start = time.perf_counter()
    timed = sys.platform == "darwin"
    actual = ["/usr/bin/time", "-l", *command] if timed else command
    result = subprocess.run(actual, cwd=ROOT, capture_output=True, timeout=30, check=False)
    elapsed = time.perf_counter() - start
    if result.returncode:
        raise RuntimeError(f"Benchmark failed with status {result.returncode}: {result.stderr.decode(errors='replace')[:1024]}")
    metrics = {"elapsed_ms": elapsed * 1000}
    if timed:
        text = result.stderr.decode(errors="replace")
        cpu = re.search(r"([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys", text)
        rss = re.search(r"(\d+)\s+maximum resident set size", text)
        if not cpu or not rss:
            raise RuntimeError("macOS time metrics missing")
        metrics.update(user_cpu_ms=float(cpu[2]) * 1000, system_cpu_ms=float(cpu[3]) * 1000, peak_rss_bytes=int(rss[1]))
    return metrics


def main():
    results = {}
    for workload, variants in COMMANDS.items():
        samples = {name: [] for name in variants}
        for command in variants.values():
            measure(command)
        for iteration in range(10):
            order = list(variants) if iteration % 2 == 0 else list(reversed(variants))
            for name in order:
                samples[name].append(measure(variants[name]))
        results[workload] = {
            name: {metric: {"median": statistics.median(sample[metric] for sample in values), "range": [min(sample[metric] for sample in values), max(sample[metric] for sample in values)], "samples": [sample[metric] for sample in values]} for metric in values[0]}
            for name, values in samples.items()
        }
    print(json.dumps({"warmups": 1, "repetitions": 10, "clock": "perf_counter", "scope": "pure planning process startup; maximum network throughput, allocations and cancellation are not measured", "results": results}, indent=2))


if __name__ == "__main__":
    main()
