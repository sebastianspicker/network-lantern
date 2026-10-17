#!/usr/bin/env bash
# runner.sh - single test run execution

# Emit a JSON object marking a failed test run (appended to the JSON log).
# Args:
#   $1 - round name
#   $2 - test type
#   $3 - target host
#   $4 - optional raw command output
#   $5 - optional error text (first non-empty mtr stderr line)
# Output/Returns:
#   Prints a one-line JSON object with _failed:true to stdout
append_failed_marker() {
  local round=$1
  local type=$2
  local host=$3
  local raw_output=${4:-}
  local error_text=${5:-}
  local extra=""
  if [[ -n "$error_text" ]]; then
    extra=$(printf ',"_error":"%s"' "$(json_escape "$error_text")")
  fi
  if [[ -n "$raw_output" ]]; then
    printf '{"_failed":true,"round":"%s","type":"%s","host":"%s"%s,"raw_output":"%s"}\n' \
      "$(json_escape "$round")" \
      "$(json_escape "$type")" \
      "$(json_escape "$host")" \
      "$extra" \
      "$(json_escape "$raw_output")"
  else
    printf '{"_failed":true,"round":"%s","type":"%s","host":"%s"%s}\n' \
      "$(json_escape "$round")" \
      "$(json_escape "$type")" \
      "$(json_escape "$host")" \
      "$extra"
  fi
}

# Print the first non-empty line of the captured mtr stderr (trimmed).
_first_mtr_error_line() {
  local line
  [[ -s "${CURRENT_ERR:-}" ]] || return 0
  while IFS= read -r line || [[ -n "$line" ]]; do
    line=$(trim "$line")
    if [[ -n "$line" ]]; then
      printf '%s' "$line"
      return 0
    fi
  done <"$CURRENT_ERR"
  return 0
}

# Detect a privilege failure and, once per run, print a fix hint and stop probing.
# Args:
#   $1 - first mtr stderr line
# Side effects:
#   Sets ABORT_RUN=1 and prints one ERROR: hint to stderr the first time.
_handle_mtr_permission_error() {
  local error_text=$1
  case "$error_text" in
    *"Failure to open"* | *"Operation not permitted"* | *"Permission denied"*) ;;
    *) return 0 ;;
  esac
  # shellcheck disable=SC2034
  ABORT_RUN=1
  if ((${PERMISSION_HINT_SHOWN:-0})); then
    return 0
  fi
  PERMISSION_HINT_SHOWN=1
  if [[ "$(uname -s)" == "Darwin" ]]; then
    echo "ERROR: mtr needs root: run with sudo, or make mtr-packet setuid root (Homebrew: sudo chown root \"\$(command -v mtr-packet)\" && sudo chmod 4755 \"\$(command -v mtr-packet)\")" >&2
  else
    echo "ERROR: mtr-packet needs CAP_NET_RAW: sudo setcap cap_net_raw+ep \"\$(command -v mtr-packet)\" or run with sudo" >&2
  fi
}

_terminate_mtr_at_deadline() {
  local mtr_pid=$1

  kill -TERM "$mtr_pid" 2>/dev/null || true
  for _ in {1..10}; do
    if ! kill -0 "$mtr_pid" 2>/dev/null; then
      return
    fi
    sleep 0.1
  done
  if kill -0 "$mtr_pid" 2>/dev/null; then
    kill -KILL "$mtr_pid" 2>/dev/null || true
  fi
}

_capture_mtr_with_deadline() {
  local host=$1
  shift
  local mtr_pid
  local timed_out=0
  local remaining_ticks=$((MTR_TIMEOUT_SECONDS * 2))

  mtr "$@" -- "$host" >"$CURRENT_TMP" 2>"$CURRENT_ERR" &
  mtr_pid=$!
  CURRENT_MTR_PID=$mtr_pid

  while kill -0 "$mtr_pid" 2>/dev/null; do
    if ((remaining_ticks <= 0)); then
      timed_out=1
      _terminate_mtr_at_deadline "$mtr_pid"
      break
    fi
    sleep 0.5
    ((remaining_ticks--)) || true
  done

  local mtr_status=0
  if wait "$mtr_pid"; then
    mtr_status=0
  else
    mtr_status=$?
  fi
  # shellcheck disable=SC2034
  CURRENT_MTR_PID=""
  if [[ -s "$CURRENT_ERR" ]]; then
    cat -- "$CURRENT_ERR" >>"$TABLE_LOG"
  fi
  ((timed_out)) && return 124
  return "$mtr_status"
}

_append_valid_mtr_output() {
  [[ -s "$CURRENT_TMP" ]] || return 1
  jq -e -s 'length == 1 and (.[0] | type == "object")' "$CURRENT_TMP" >/dev/null 2>&1 || return 1

  cat "$CURRENT_TMP" >>"$JSON_LOG"
  printf '\n' >>"$JSON_LOG"
}

_read_current_mtr_output() {
  [[ -s "$CURRENT_TMP" ]] && head -c 4096 -- "$CURRENT_TMP"
  return 0
}

_record_invalid_mtr_output() {
  local round=$1
  local type=$2
  local host=$3
  local invalid_reason="invalid JSON output"
  local invalid_output

  [[ -s "$CURRENT_TMP" ]] || invalid_reason="empty output"
  invalid_output=$(_read_current_mtr_output)
  log_line WARN "mtr produced $invalid_reason for round=$round type=$type host=$host"
  append_failed_marker "$round" "$type" "$host" "$invalid_output" >>"$JSON_LOG"
  ((RUN_FAIL++)) || true
  log_line FAIL "round=$round type=$type host=$host ($invalid_reason)"
}

_record_mtr_failure() {
  local round=$1
  local type=$2
  local host=$3
  local mtr_status=$4
  local raw_output error_text

  raw_output=$(_read_current_mtr_output)
  error_text=$(_first_mtr_error_line)
  append_failed_marker "$round" "$type" "$host" "$raw_output" "$error_text" >>"$JSON_LOG"
  ((RUN_FAIL++)) || true
  if ((mtr_status == 124)); then
    log_line FAIL "round=$round type=$type host=$host (timeout after ${MTR_TIMEOUT_SECONDS}s)"
  else
    log_line FAIL "round=$round type=$type host=$host (exit=$mtr_status)"
  fi
  _handle_mtr_permission_error "$error_text"
}

_cleanup_current_run() {
  rm -f -- "$CURRENT_TMP" "$CURRENT_ERR"
  CURRENT_TMP=""
  CURRENT_ERR=""
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
  CURRENT_ERR=$(mktemp "${TMPDIR:-/tmp}/mtr-suite-err.XXXXXXXX")
  local mtr_status=0
  if _capture_mtr_with_deadline "$host" "${local_mtr_args[@]}" "${local_extra_args[@]}"; then
    :
  else
    mtr_status=$?
    _record_mtr_failure "$round" "$type" "$host" "$mtr_status"
    _cleanup_current_run
    return
  fi

  if ! _append_valid_mtr_output; then
    _record_invalid_mtr_output "$round" "$type" "$host"
    _cleanup_current_run
    return
  fi

  if ((DO_SUMMARY)) && ! summarize_json "$CURRENT_TMP"; then
    log_line WARN "summary failed for round=$round type=$type host=$host"
  fi

  ((RUN_OK++)) || true
  log_line OK "round=$round type=$type host=$host"
  _cleanup_current_run
}
