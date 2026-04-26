#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "Initializing test submodules..."
if ! command -v git >/dev/null 2>&1; then
  echo "git not found. Unable to initialize submodules." >&2
  exit 1
fi

if [[ -f "$REPO_ROOT/.gitmodules" ]]; then
  git submodule update --init --recursive
else
  echo "No .gitmodules file found; skipping submodule initialization."
fi

if [[ -x "$REPO_ROOT/tests/bats/bin/bats" ]]; then
  echo "Vendored bats detected at: tests/bats/bin/bats"
else
  echo "Vendored bats not found. Install bats via your package manager if needed." >&2
fi

echo "Test dependencies installed."
echo "Run tests with: make test-bash"
