#!/usr/bin/env bash
# common.sh - shared utility functions

# Print an error message to stderr and exit with status 1.
# Args:
#   $@ - error message text
# Output/Returns:
#   Prints "ERROR: <message>" to stderr; exits 1
die() {
  echo "ERROR: $*" >&2
  exit 1
}

# Abort unless running under Bash 4+.
# Output/Returns:
#   Calls die() if BASH_VERSION is unset or major version < 4
require_bash4() {
  if [[ -z "${BASH_VERSION:-}" ]]; then
    die "Please run this script with bash: bash $0"
  fi
  if ((BASH_VERSINFO[0] < 4)); then
    die "Bash 4+ required (found: ${BASH_VERSION})."
  fi
}

# Abort unless the given command exists in PATH.
# Args:
#   $1 - command name to look up
# Output/Returns:
#   Calls die() if command is not found; includes install hints for known deps
require_cmd() {
  command -v "$1" >/dev/null 2>&1 && return 0
  local hint=""
  case "$1" in
    mtr) hint="Install: brew install mtr (macOS) or apt-get install mtr (Debian/Ubuntu)" ;;
    jq) hint="Install: brew install jq (macOS) or apt-get install jq (Debian/Ubuntu)" ;;
    column) hint="Install: brew install util-linux (macOS) or apt-get install bsdmainutils (Debian/Ubuntu)" ;;
  esac
  if [[ -n "$hint" ]]; then
    die "Missing dependency: $1. $hint"
  else
    die "Missing dependency: $1"
  fi
}

# Strip leading and trailing whitespace from a string.
# Args:
#   $1 - input string
# Output/Returns:
#   Prints trimmed string to stdout (no trailing newline)
trim() {
  local v=$1
  v="${v#"${v%%[![:space:]]*}"}"
  v="${v%"${v##*[![:space:]]}"}"
  printf '%s' "$v"
}

# Check whether a value exists in a list of arguments.
# Args:
#   $1 - needle to search for
#   $@ - remaining args form the haystack
# Output/Returns:
#   Returns 0 if found, 1 otherwise
contains_item() {
  local needle=$1
  shift
  local item
  for item in "$@"; do
    if [[ "$item" == "$needle" ]]; then
      return 0
    fi
  done
  return 1
}

# Print each argument on its own line.
# Args:
#   $@ - items to print
# Output/Returns:
#   One item per line on stdout
print_list() {
  local item
  for item in "$@"; do
    echo "$item"
  done
}

# Escape a string for safe embedding in a JSON value (no surrounding quotes).
# Args:
#   $1 - raw string
# Output/Returns:
#   Prints the escaped string to stdout
json_escape() {
  local s=$1
  s=${s//\\/\\\\}
  s=${s//\"/\\\"}
  s=${s//$'\n'/\\n}
  s=${s//$'\r'/\\r}
  s=${s//$'\t'/\\t}
  printf '%s' "$s"
}
