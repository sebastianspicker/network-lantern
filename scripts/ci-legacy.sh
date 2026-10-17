#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)

# Same patterns as scripts/Invoke-SecretScan.ps1 (keep the two lists in sync).
SECRET_PATTERNS=(
  '-----BEGIN (RSA|DSA|EC|OPENSSH|PGP|ENCRYPTED) PRIVATE KEY-----'
  'AKIA[0-9A-Z]{16}'
  'ASIA[0-9A-Z]{16}'
  'ghp_[A-Za-z0-9]{36}'
  'gho_[A-Za-z0-9]{36}'
  'ghu_[A-Za-z0-9]{36}'
  'ghs_[A-Za-z0-9]{36}'
  'ghr_[A-Za-z0-9]{36}'
  'github_pat_[A-Za-z0-9_]{22,}'
  'xox[baprs]-[A-Za-z0-9-]{10,48}'
  'sk-[A-Za-z0-9]{20,}'
  'AIza[A-Za-z0-9_\\-]{35}'
  'eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}'
  'npm_[A-Za-z0-9]{36}'
  'pypi-[A-Za-z0-9]{60,}'
  'SG\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9_-]{43}'
  'sk_live_[A-Za-z0-9]{24,}'
  'rk_live_[A-Za-z0-9]{24,}'
)

# Count secret-pattern hits in one file, like Invoke-SecretScan.ps1: once per
# matching pattern per line, skipping the scanner itself and binary extensions.
# Args:
#   $1 - file path
# Output/Returns:
#   Prints the hit count to stdout (never the matched text)
secret_hits_in_file() {
  local file=$1 pattern count total=0
  case "$file" in
    */Invoke-SecretScan.ps1 | *.exe | *.dll | *.zip | *.gz | *.tar | *.png | *.jpg | *.gif | *.ico | *.woff | *.woff2 | *.ttf | *.eot | *.pdf)
      echo 0
      return 0
      ;;
  esac
  if [[ -f "$file" ]]; then
    for pattern in "${SECRET_PATTERNS[@]}"; do
      count=$(grep -aEic -e "$pattern" -- "$file" || true)
      total=$((total + count))
    done
  fi
  echo "$total"
}

# Grep-based fallback for scripts/Invoke-SecretScan.ps1 (no PowerShell needed).
# Args:
#   $@ - optional files to scan instead of the Git file list (for testing)
# Returns:
#   0 when clean, 1 when potential secrets are found; prints only the count
# shellcheck disable=SC2120
secret_scan_fallback() {
  local hits=0 file count
  if (($# > 0)); then
    for file in "$@"; do
      count=$(secret_hits_in_file "$file")
      hits=$((hits + count))
    done
  else
    while IFS= read -r -d '' file; do
      count=$(secret_hits_in_file "$REPO_ROOT/$file")
      hits=$((hits + count))
    done < <(git -C "$REPO_ROOT" ls-files -z --cached --others --exclude-standard)
  fi
  if ((hits > 0)); then
    echo "Potential secrets detected ($hits). Review locally; output suppressed." >&2
    return 1
  fi
}

# Allow sourcing for tests of the fallback scan.
if [[ "${BASH_SOURCE[0]}" != "$0" ]]; then
  return 0
fi

if ! command -v shellcheck >/dev/null 2>&1; then
  echo "shellcheck not found. Install shellcheck first." >&2
  exit 1
fi

make -C "$REPO_ROOT" lint

if ! command -v pwsh >/dev/null 2>&1; then
  secret_scan_fallback
  echo "PowerShell checks (PSScriptAnalyzer, PowerShell secret scan) SKIPPED: pwsh not installed" >&2
  if [[ ${NETWORK_LANTERN_ALLOW_PARTIAL_GATE:-0} == 1 ]]; then
    exit 0
  fi
  exit 3
fi

pwsh -NoProfile -NonInteractive -File "$REPO_ROOT/scripts/Invoke-SecretScan.ps1"
ci_args=(-NoInstall)
if [[ ${NETWORK_LANTERN_INSTALL_MISSING_MODULES:-0} == 1 ]]; then
  ci_args=()
fi
pwsh -NoProfile -NonInteractive -File "$REPO_ROOT/scripts/ci.ps1" "${ci_args[@]}"
