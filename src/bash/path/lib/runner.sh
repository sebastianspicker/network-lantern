#!/usr/bin/env bash
# runner.sh - single test run execution

# shellcheck disable=SC1091
source "${BASH_SOURCE[0]%/*}/common.sh" 2>/dev/null || true
# shellcheck disable=SC1091
source "${BASH_SOURCE[0]%/*}/mtr_args.sh" 2>/dev/null || true
# shellcheck disable=SC1091
source "${BASH_SOURCE[0]%/*}/logging.sh" 2>/dev/null || true

# Emit a JSON object marking a failed test run (appended to the JSON log).
# Args:
#   $1 - round name
#   $2 - test type
#   $3 - target host
# Output/Returns:
#   Prints a one-line JSON object with _failed:true to stdout
append_failed_marker() {
  local round=$1
  local type=$2
  local host=$3
  printf '{"_failed":true,"round":"%s","type":"%s","host":"%s"}\n' \
    "$(json_escape "$round")" \
    "$(json_escape "$type")" \
    "$(json_escape "$host")"
}

# Run a single MTR test, log results, and update counters.
# Args:
#   $1 - round name
#   $2 - test type (e.g. ICMP4, TCP6)
#   $3 - target host
#   $4 - 1-based run index (for progress display)
# Side effects:
#   Appends output to JSON_LOG and TABLE_LOG; increments RUN_OK or RUN_FAIL.
#   In DRY_RUN mode, only logs the planned command.
execute_single_run() {
  local round=$1
  local type=$2
  local host=$3
  local run_index=$4

  local -a local_mtr_args=()
  local -a local_extra_args=()

  set_round_extra_args "$round"
  # shellcheck disable=SC2154
  local_extra_args=("${extra_args[@]}")

  set_mtr_args_for_type "$type"
  # shellcheck disable=SC2154
  local_mtr_args=("${mtr_args[@]}")

  log_line INFO "RUN [$run_index/$TOTAL_RUNS] round=$round type=$type host=$host"

  if ((DRY_RUN)); then
    log_line PLAN "mtr ${local_mtr_args[*]} ${local_extra_args[*]} $host"
    return 0
  fi

  CURRENT_TMP=$(mktemp "${TMPDIR:-/tmp}/mtr-suite.XXXXXXXX")

  if mtr "${local_mtr_args[@]}" "${local_extra_args[@]}" -- "$host" >"$CURRENT_TMP" 2>>"$TABLE_LOG"; then
    # Validate mtr produced non-empty output before appending
    if [[ -s "$CURRENT_TMP" ]]; then
      cat "$CURRENT_TMP" >>"$JSON_LOG"
      printf '\n' >>"$JSON_LOG"
    else
      log_line WARN "mtr produced empty output for round=$round type=$type host=$host"
      append_failed_marker "$round" "$type" "$host" >>"$JSON_LOG"
      ((RUN_FAIL++)) || true
      log_line FAIL "round=$round type=$type host=$host (empty output)"
      rm -f -- "$CURRENT_TMP"
      CURRENT_TMP=""
      return
    fi

    if ((DO_SUMMARY)); then
      if ! summarize_json "$CURRENT_TMP"; then
        log_line WARN "summary failed for round=$round type=$type host=$host"
      fi
    fi

    ((RUN_OK++)) || true
    log_line OK "round=$round type=$type host=$host"
  else
    {
      append_failed_marker "$round" "$type" "$host"
      if [[ -s "$CURRENT_TMP" ]]; then
        cat "$CURRENT_TMP"
      fi
      printf '\n'
    } >>"$JSON_LOG"

    ((RUN_FAIL++)) || true
    log_line FAIL "round=$round type=$type host=$host"
  fi

  rm -f -- "$CURRENT_TMP"
  CURRENT_TMP=""
}
