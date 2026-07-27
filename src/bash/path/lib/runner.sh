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
#   $4 - optional raw command output
# Output/Returns:
#   Prints a one-line JSON object with _failed:true to stdout
append_failed_marker() {
  local round=$1
  local type=$2
  local host=$3
  local raw_output=${4:-}
  if [[ -n "$raw_output" ]]; then
    printf '{"_failed":true,"round":"%s","type":"%s","host":"%s","raw_output":"%s"}\n' \
      "$(json_escape "$round")" \
      "$(json_escape "$type")" \
      "$(json_escape "$host")" \
      "$(json_escape "$raw_output")"
  else
    printf '{"_failed":true,"round":"%s","type":"%s","host":"%s"}\n' \
      "$(json_escape "$round")" \
      "$(json_escape "$type")" \
      "$(json_escape "$host")"
  fi
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
  local mtr_pid
  local mtr_status=0
  local timed_out=0
  local deadline

  mtr "${local_mtr_args[@]}" "${local_extra_args[@]}" -- "$host" >"$CURRENT_TMP" 2>>"$TABLE_LOG" &
  mtr_pid=$!
  CURRENT_MTR_PID=$mtr_pid

  deadline=$((SECONDS + MTR_TIMEOUT_SECONDS))
  while kill -0 "$mtr_pid" 2>/dev/null; do
    if ((SECONDS >= deadline)); then
      timed_out=1
      kill -TERM "$mtr_pid" 2>/dev/null || true
      for _ in {1..10}; do
        if ! kill -0 "$mtr_pid" 2>/dev/null; then
          break
        fi
        sleep 0.1
      done
      if kill -0 "$mtr_pid" 2>/dev/null; then
        kill -KILL "$mtr_pid" 2>/dev/null || true
      fi
      break
    fi
    sleep 0.1
  done

  if wait "$mtr_pid"; then
    mtr_status=0
  else
    mtr_status=$?
  fi
  # shellcheck disable=SC2034
  CURRENT_MTR_PID=""

  if ((timed_out)); then
    mtr_status=124
  fi

  if ((mtr_status == 0)); then
    # Validate mtr produced one non-empty JSON object before appending.
    if [[ -s "$CURRENT_TMP" ]] && jq -e -s 'length == 1 and (.[0] | type == "object")' "$CURRENT_TMP" >/dev/null 2>&1; then
      cat "$CURRENT_TMP" >>"$JSON_LOG"
      printf '\n' >>"$JSON_LOG"
    else
      local invalid_reason="invalid JSON output"
      if [[ ! -s "$CURRENT_TMP" ]]; then
        invalid_reason="empty output"
      fi
      local invalid_output=""
      if [[ -s "$CURRENT_TMP" ]]; then
        invalid_output=$(head -c 4096 -- "$CURRENT_TMP")
      fi
      log_line WARN "mtr produced $invalid_reason for round=$round type=$type host=$host"
      append_failed_marker "$round" "$type" "$host" "$invalid_output" >>"$JSON_LOG"
      ((RUN_FAIL++)) || true
      log_line FAIL "round=$round type=$type host=$host ($invalid_reason)"
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
    local raw_output=""
    if [[ -s "$CURRENT_TMP" ]]; then
      raw_output=$(head -c 4096 -- "$CURRENT_TMP")
    fi
    append_failed_marker "$round" "$type" "$host" "$raw_output" >>"$JSON_LOG"

    ((RUN_FAIL++)) || true
    if ((mtr_status == 124)); then
      log_line FAIL "round=$round type=$type host=$host (timeout after ${MTR_TIMEOUT_SECONDS}s)"
    else
      log_line FAIL "round=$round type=$type host=$host (exit=$mtr_status)"
    fi
  fi

  rm -f -- "$CURRENT_TMP"
  CURRENT_TMP=""
}
