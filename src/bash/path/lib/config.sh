#!/usr/bin/env bash
# config.sh - host configuration loading

# shellcheck disable=SC1091
source "${BASH_SOURCE[0]%/*}/common.sh" 2>/dev/null || true
# shellcheck disable=SC1091
source "${BASH_SOURCE[0]%/*}/validation.sh" 2>/dev/null || true

# Return the absolute path to the default hosts.conf file.
# Output/Returns:
#   Prints "<repo_root>/config/hosts.conf" to stdout
default_hosts_config_path() {
  local script_dir repo_root
  script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
  repo_root=$(cd "$script_dir/../../../.." && pwd)
  echo "$repo_root/config/hosts.conf"
}

# Parse a hosts.conf file and populate HOSTS_IPV4 / HOSTS_IPV6 arrays.
# Args:
#   $1 - path to config file (key=value format, keys: ipv4, ipv6)
# Side effects:
#   Sets global arrays HOSTS_IPV4 and HOSTS_IPV6 when entries are found
load_hosts_from_config() {
  local config_path=$1
  local line raw_key raw_val key val
  local -a loaded4=()
  local -a loaded6=()

  [[ -f "$config_path" ]] || return 0

  while IFS= read -r line || [[ -n "$line" ]]; do
    line=$(trim "$line")
    [[ -n "$line" ]] || continue
    [[ "$line" == \#* ]] && continue

    if [[ "$line" != *=* ]]; then
      continue
    fi

    raw_key=${line%%=*}
    raw_val=${line#*=}
    key=$(trim "$raw_key")
    val=$(trim "$raw_val")
    key=${key,,}

    [[ -n "$val" ]] || continue

    case "$key" in
      ipv4)
        validate_host "$val"
        loaded4+=("$val")
        ;;
      ipv6)
        validate_host "$val"
        loaded6+=("$val")
        ;;
    esac
  done <"$config_path"

  if ((${#loaded4[@]} > 0)); then
    # shellcheck disable=SC2034
    HOSTS_IPV4=("${loaded4[@]}")
  fi
  if ((${#loaded6[@]} > 0)); then
    # shellcheck disable=SC2034
    HOSTS_IPV6=("${loaded6[@]}")
  fi
}
