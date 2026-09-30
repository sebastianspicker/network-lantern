#!/usr/bin/env bats

load test_helper.bash

setup() {
  setup_path_app
}

@test "timeout sends TERM before completing as a failed run" {
  install_fake_mtr mtr-timeout
  local json_log="$BATS_TEST_TMPDIR/results.json"
  export MTR_SIGNAL_FILE="$BATS_TEST_TMPDIR/mtr-signal"

  export MTR_TIMEOUT_SECONDS=1
  run_path_app --json-log "$json_log" --table-log "$BATS_TEST_TMPDIR/summary.log" --types ICMP4 --rounds Standard --hosts4 timeout.example --no-summary

  [ "$status" -eq 1 ]
  [ "$(<"$MTR_SIGNAL_FILE")" = "TERM" ]
  [ "$(jq -sr '.[0]._failed' "$json_log")" = "true" ]
  [[ "$output" == *"timeout after 1s"* ]]
}

@test "TERM interrupts a running path command with status 143" {
  install_fake_mtr mtr-timeout
  local output_file="$BATS_TEST_TMPDIR/term-output"
  local json_log="$BATS_TEST_TMPDIR/results.json"

  bash -c 'trap - INT; exec bash "$@"' bash "$PATH_APP" --json-log "$json_log" --table-log "$BATS_TEST_TMPDIR/summary.log" --types ICMP4 --rounds Standard --hosts4 signal.example --no-summary >"$output_file" 2>&1 &
  local app_pid=$!
  sleep 0.3
  SECONDS=0
  kill -TERM "$app_pid"
  local app_status=0
  wait "$app_pid" || app_status=$?

  [ "$app_status" -eq 143 ]
  [ "$SECONDS" -le 2 ]
  [[ "$(<"$output_file")" == *"Interrupted (SIGTERM)"* ]]
}

@test "normal polling preserves successful and failed child status with half-second sleeps" {
  local runner="$BATS_TEST_DIRNAME/../../../src/bash/path/lib/runner.sh"
  for child_status in 0 17; do
    run bash -c '
      source "$1"
      CURRENT_TMP="$2/output"
      TABLE_LOG="$2/errors"
      MTR_TIMEOUT_SECONDS=10
      mtr() { command sleep 0.1; return "$3"; }
      sleep() { printf "%s\n" "$1" >> "$CURRENT_TMP.polls"; command sleep "$1"; }
      : > "$CURRENT_TMP.polls"
      SECONDS=0
      _capture_mtr_with_deadline fixture.example unused unused "$3"
      result=$?
      [[ $SECONDS -le 2 ]] || exit 99
      cat "$CURRENT_TMP.polls"
      exit "$result"
    ' bash "$runner" "$BATS_TEST_TMPDIR" "$child_status"
    [ "$status" -eq "$child_status" ]
    [ "$output" = "0.5" ]
  done
}

@test "one-second timeout uses two normal polls and preserves ten TERM grace checks" {
  local runner="$BATS_TEST_DIRNAME/../../../src/bash/path/lib/runner.sh"
  run bash -c '
    source "$1"
    CURRENT_TMP="$2/output"
    TABLE_LOG="$2/errors"
    MTR_TIMEOUT_SECONDS=1
    mtr() { trap "" TERM; while :; do command sleep 0.1; done; }
    sleep() { printf "%s\n" "$1" >> "$CURRENT_TMP.polls"; command sleep "$1"; }
    _capture_mtr_with_deadline fixture.example
  ' bash "$runner" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 124 ]
  mapfile -t polls < "$BATS_TEST_TMPDIR/output.polls"
  [ "${#polls[@]}" -eq 12 ]
  [ "${polls[0]}" = "0.5" ]
  [ "${polls[1]}" = "0.5" ]
  for poll in "${polls[@]:2}"; do
    [ "$poll" = "0.1" ]
  done
}

@test "INT cleanup trap exits with status 130" {
  local loader="$BATS_TEST_DIRNAME/../../../src/bash/path/load.sh"

  run bash -c 'source "$1"; path_install_cleanup_traps; kill -INT "$$"; printf unreachable' bash "$loader"

  [ "$status" -eq 130 ]
  [[ "$output" == *"Interrupted (SIGINT)"* ]]
}
