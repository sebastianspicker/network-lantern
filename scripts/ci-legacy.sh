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

make -C "$REPO_ROOT" lint

pwsh -NoProfile -NonInteractive -File "$REPO_ROOT/scripts/Invoke-SecretScan.ps1"
ci_args=(-NoInstall)
if [[ ${NETWORK_LANTERN_INSTALL_MISSING_MODULES:-0} == 1 ]]; then
  ci_args=()
fi
pwsh -NoProfile -NonInteractive -File "$REPO_ROOT/scripts/ci.ps1" "${ci_args[@]}"
