#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)

if ! command -v pwsh >/dev/null 2>&1; then
  echo "pwsh not found. Install PowerShell 7+ first." >&2
  exit 1
fi

if ! command -v shellcheck >/dev/null 2>&1; then
  echo "shellcheck not found. Install shellcheck first." >&2
  exit 1
fi

shellcheck -x \
  "$REPO_ROOT/apps/path/mtr-test-suite.sh" \
  "$REPO_ROOT/scripts/ci-local.sh" \
  "$REPO_ROOT/scripts/install-test-deps.sh" \
  "$REPO_ROOT/scripts/run-workflow.sh" \
  "$REPO_ROOT"/src/bash/path/lib/*.sh

if command -v bats >/dev/null 2>&1; then
  bats "$REPO_ROOT/tests/path"
else
  echo "bats not found. Skipping Bash test suite." >&2
fi

pwsh -NoProfile -NonInteractive -File "$REPO_ROOT/scripts/ci.ps1" -NoInstall
