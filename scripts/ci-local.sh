#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
BATS_BIN="${BATS:-}"

if [[ -z "$BATS_BIN" ]]; then
  if command -v bats >/dev/null 2>&1; then
    BATS_BIN="bats"
  elif [[ -x "$REPO_ROOT/tests/bats/bin/bats" ]]; then
    BATS_BIN="$REPO_ROOT/tests/bats/bin/bats"
  fi
fi

HAS_PWSH=1
if ! command -v pwsh >/dev/null 2>&1; then
  HAS_PWSH=0
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

if [[ -n "$BATS_BIN" ]]; then
  "$BATS_BIN" "$REPO_ROOT/tests/path"
else
  echo "bats not found. Skipping Bash test suite. Install bats or run scripts/install-test-deps.sh." >&2
fi

if [[ "$HAS_PWSH" -eq 1 ]]; then
  pwsh -NoProfile -NonInteractive -File "$REPO_ROOT/scripts/ci.ps1"
else
  echo "pwsh not found. Bash checks were run, but PowerShell validation could not run." >&2
  exit 1
fi
