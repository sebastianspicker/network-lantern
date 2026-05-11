#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

if [[ -f .gitmodules ]] && git config -f .gitmodules --get-regexp '^submodule\..*\.path$' >/dev/null 2>&1; then
	echo "Initializing test submodules..."
	git submodule update --init --recursive
else
	echo "No test submodules are configured; skipping submodule initialization."
fi

echo "Install shellcheck and bats with your platform package manager, or provide them on PATH."
echo "Run tests with: bats tests/path"
