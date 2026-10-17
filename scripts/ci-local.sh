#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
# ci-legacy.sh exits 3 when PowerShell checks were skipped; still run the Rust
# gate, then pass the legacy status through.
legacy_status=0
"$REPO_ROOT/scripts/ci-legacy.sh" || legacy_status=$?
if ((legacy_status != 0 && legacy_status != 3)); then
  exit "$legacy_status"
fi
"$REPO_ROOT/scripts/ci-rust.sh"
exit "$legacy_status"
