#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO_ROOT"
node --test tests/architecture/*.test.cjs
npm --prefix desktop run check
npm --prefix desktop test
npm --prefix desktop run build
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --locked -p network-lantern
npm --prefix desktop run test:browser
npm --prefix desktop run build:e2e
if [[ $(uname -s) == Linux && -z ${DISPLAY:-} ]]; then
  xvfb-run -a npm --prefix desktop run test:native
else
  npm --prefix desktop run test:native
fi
npm --prefix desktop run build
cargo build --locked -p network-lantern-desktop -p lantern-helper
git diff --check
