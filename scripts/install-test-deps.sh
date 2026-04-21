#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "Initializing test submodules..."
git submodule update --init --recursive tests/bats tests/helpers/bats-support tests/helpers/bats-assert

echo "Test dependencies installed."
echo "Run tests with: tests/bats/bin/bats tests/"
