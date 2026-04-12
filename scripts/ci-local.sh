#!/usr/bin/env bash
set -euo pipefail

if ! command -v pwsh >/dev/null 2>&1; then
  echo "pwsh not found. Install PowerShell 7+ first." >&2
  exit 1
fi

pwsh -NoProfile -NonInteractive -File "$(cd "$(dirname "$0")/.." && pwd)/scripts/ci.ps1"
