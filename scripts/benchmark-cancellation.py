#!/usr/bin/env python3
"""Compare cancellation of an identical stalled loopback iperf3 control exchange."""
import json
import os
from pathlib import Path
import signal
import socket
import statistics
import subprocess
import tempfile
import threading
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
ARTIFACTS = ROOT / "artifacts/rust-migration"
REFERENCE = ARTIFACTS / "legacy-reference/src/powershell/throughput/NetworkLantern.Throughput.psm1"


def measure(variant, directory, wrapper):
    cookie_received = threading.Event()
    stop = threading.Event()
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(4)
    listener.settimeout(0.2)
    port = listener.getsockname()[1]

    def serve():
        while not stop.is_set():
            try:
                peer, _ = listener.accept()
            except socket.timeout:
                continue
            with peer:
                peer.settimeout(0.2)
                cookie = b""
                while not stop.is_set():
                    try:
                        part = peer.recv(37 - len(cookie))
                    except socket.timeout:
                        continue
                    if not part:
                        break
                    cookie += part
                    if len(cookie) == 37:
                        cookie_received.set()
                        stop.wait(10)
                        break

    server = threading.Thread(target=serve, daemon=True)
    server.start()
    run_id, nonce = uuid.uuid4().hex, uuid.uuid4().hex
    cancel_file = directory / f"cancel-{run_id}"
    if variant == "legacy":
        command = ["pwsh", "-NoProfile", "-NonInteractive", "-File", str(wrapper),
                   str(REFERENCE), "/opt/homebrew/bin/iperf3", str(port),
                   str(cancel_file), run_id, nonce]
    else:
        command = [str(ROOT / "target/release/network-lantern"), "throughput", "--target", "127.0.0.1",
                   "--port", str(port), "--single-test", "--protocol", "tcp", "--duration", "1",
                   "--omit", "0", "--settings", '{"skip_reachability_check":true,"disable_mtu_probe":true}',
                   "--out", str(directory / run_id)]
    process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        if not cookie_received.wait(15):
            raise RuntimeError(f"{variant} did not reach the control exchange")
        start = time.monotonic_ns()
        if variant == "legacy":
            cancel_file.write_text(f"NETWORK-LANTERN-IPERF3-CANCEL/1:{run_id}:{nonce}")
        else:
            process.send_signal(signal.SIGINT)
        while True:
            pid, status, usage = os.wait4(process.pid, os.WNOHANG)
            if pid:
                process.returncode = os.waitstatus_to_exitcode(status)
                break
            if time.monotonic_ns() - start > 10_000_000_000:
                raise RuntimeError(f"{variant} cancellation exceeded 10 seconds")
            time.sleep(0.001)
        latency = (time.monotonic_ns() - start) / 1_000_000
        expected = 0 if variant == "legacy" else 130
        if process.returncode != expected:
            raise RuntimeError(f"{variant} returned {process.returncode}, expected {expected}")
        return {"cancellation_ms": latency, "cpu_seconds": usage.ru_utime + usage.ru_stime,
                "max_rss_bytes": usage.ru_maxrss}
    finally:
        if process.returncode is None:
            process.kill()
            process.wait()
        stop.set()
        server.join(2)
        listener.close()


def main():
    if os.uname().sysname != "Darwin":
        raise SystemExit("This benchmark records macOS wait4 RSS units and the local oracle path")
    ARTIFACTS.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="cancellation-", dir=ARTIFACTS) as temporary:
        directory = Path(temporary).resolve()
        wrapper = directory / "cancel.ps1"
        wrapper.write_text('''param($ModulePath,$Oracle,[int]$Port,$CancelFile,$RunId,$Nonce)
$module = Import-Module $ModulePath -Force -PassThru
& $module {
  param($Oracle,$Port,$CancelFile,$RunId,$Nonce)
  $result = Invoke-Iperf3NativeProcess -FilePath $Oracle -Arguments @('-c','127.0.0.1','-p',"$Port",'-t','1','-J') -TimeoutMs 20000 -CancellationFile $CancelFile -CancellationRunId $RunId -CancellationNonce $Nonce
  if (-not $result.Cancelled) { throw 'Cancellation was not observed' }
} $Oracle $Port $CancelFile $RunId $Nonce
''')
        samples = {"legacy": [], "rust": []}
        for variant in samples:
            measure(variant, directory, wrapper)
        for iteration in range(10):
            for variant in list(samples) if iteration % 2 == 0 else reversed(samples):
                samples[variant].append(measure(variant, directory, wrapper))
        output = {variant: {key: {"median": statistics.median(s[key] for s in rows),
                                  "range": [min(s[key] for s in rows), max(s[key] for s in rows)]}
                            for key in rows[0]} for variant, rows in samples.items()}
        print(json.dumps({"warmups": 1, "repetitions": 10,
                          "workload": "37-byte iperf3 cookie then stalled loopback control; no data traffic; legacy process wrapper vs Rust CLI with TCP-only preflight",
                          "results": output}, indent=2))


if __name__ == "__main__":
    main()
