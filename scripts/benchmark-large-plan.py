#!/usr/bin/env python3
"""Compare large pure previews through the public module and Rust CLI, using identical inputs."""
import importlib.util
import json
import statistics
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("preview", ROOT / "scripts/benchmark-rust.py")
preview = importlib.util.module_from_spec(spec)
spec.loader.exec_module(preview)


def main():
    directory = ROOT / "artifacts/rust-migration/benchmark-inputs"
    directory.mkdir(parents=True, exist_ok=True)
    fixture = directory / "large-plan.json"
    fixture.write_text(json.dumps({"Target": "fixture.invalid", "TcpStreams": [1] * 128,
                                   "TcpWindows": ["default"] * 128, "MaxTotalTests": 0}))
    wrapper = directory / "large-plan.ps1"
    wrapper.write_text('''param($ModulePath,$FixturePath)
Import-Module $ModulePath -Force
$parameters = Get-Content -LiteralPath $FixturePath -Raw | ConvertFrom-Json -AsHashtable
Measure-NetworkThroughput @parameters -WhatIf -Quiet
''')
    variants = {
        "legacy": ["pwsh", "-NoProfile", "-NonInteractive", "-File", str(wrapper),
                   str(ROOT / "artifacts/rust-migration/legacy-reference/src/powershell/throughput/NetworkLantern.Throughput.psm1"), str(fixture)],
        "rust": [str(preview.RUST), "throughput", "--config", str(fixture), "--dry-run", "--json"],
    }
    samples = {name: [] for name in variants}
    for command in variants.values():
        preview.measure(command)
    for iteration in range(10):
        for name in list(variants) if iteration % 2 == 0 else reversed(variants):
            samples[name].append(preview.measure(variants[name]))
    results = {name: {metric: {"median": statistics.median(s[metric] for s in rows),
                              "range": [min(s[metric] for s in rows), max(s[metric] for s in rows)]}
                      for metric in rows[0]} for name, rows in samples.items()}
    print(json.dumps({"warmups": 1, "repetitions": 10,
                      "workload": "128 repeated TCP stream/window entries; same sanitized parameters; public legacy module vs Rust CLI; no networking",
                      "limitation": "The legacy adapter rejected array-valued ConfigurationPath input with status 11; this workload invokes its public module with a typed parameter map. The ordinary startup benchmark still uses the original adapter.",
                      "results": results}, indent=2))


if __name__ == "__main__":
    main()
