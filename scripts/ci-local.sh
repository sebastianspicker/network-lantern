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

if ! command -v bats >/dev/null 2>&1; then
  echo "bats not found. Install bats first; the local gate requires the Bash test suite." >&2
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 not found. Install Python 3 first; the Bash regression tests require it." >&2
  exit 1
fi

shellcheck -x \
  "$REPO_ROOT/apps/path/mtr-test-suite.sh" \
  "$REPO_ROOT/scripts/ci-local.sh" \
  "$REPO_ROOT/scripts/install-test-deps.sh" \
  "$REPO_ROOT/scripts/run-workflow.sh" \
  "$REPO_ROOT"/src/bash/path/lib/*.sh

bats "$REPO_ROOT/tests/path"

pwsh -NoProfile -NonInteractive -File "$REPO_ROOT/scripts/Invoke-SecretScan.ps1"
ci_args=(-NoInstall)
if [[ ${NDS_INSTALL_MISSING_MODULES:-0} == 1 ]]; then
  ci_args=()
fi
pwsh -NoProfile -NonInteractive -File "$REPO_ROOT/scripts/ci.ps1" "${ci_args[@]}"
