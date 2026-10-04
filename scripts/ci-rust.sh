#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO_ROOT"
npm --prefix desktop run check
npm --prefix desktop run build
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --locked -p network-lantern
npm --prefix desktop run build
cargo build --locked -p network-lantern-desktop -p lantern-helper
git diff --check
