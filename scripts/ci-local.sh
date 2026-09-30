#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
"$REPO_ROOT/scripts/ci-legacy.sh"
"$REPO_ROOT/scripts/ci-rust.sh"
