#!/usr/bin/env bats
load test_helper

setup() {
  source "$PROJECT_ROOT/src/bash/path/lib/runner.sh"
  TEST_RUN_DIR=$(mktemp -d)
  JSON_LOG="$TEST_RUN_DIR/results.json.log"
  TABLE_LOG="$TEST_RUN_DIR/table.log"
  : >"$JSON_LOG"
  : >"$TABLE_LOG"
  TOTAL_RUNS=1
  DRY_RUN=0
  DO_SUMMARY=0
  RUN_OK=0
  RUN_FAIL=0
  CURRENT_TMP=""
  # shellcheck disable=SC2034
  CURRENT_MTR_PID=""
  MTR_TIMEOUT_SECONDS=5
}

teardown() {
  rm -rf "$TEST_RUN_DIR"
}

@test "failed mtr output keeps JSON log parseable" {
  # shellcheck disable=SC2329
  mtr() {
    printf 'not json\nsecond line\n'
    return 1
  }

  execute_single_run "Standard" "ICMP4" "example.com" 1

  [ "$RUN_FAIL" -eq 1 ]
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    printf '%s\n' "$line" | jq -e . >/dev/null
  done <"$JSON_LOG"
}

@test "successful mtr with invalid JSON is recorded as failed" {
  # shellcheck disable=SC2329
  mtr() {
    printf 'not json\n'
    return 0
  }

  execute_single_run "Standard" "ICMP4" "example.com" 1

  [ "$RUN_OK" -eq 0 ]
  [ "$RUN_FAIL" -eq 1 ]
  jq -e '._failed == true' "$JSON_LOG" >/dev/null
}

@test "hung mtr is terminated at the per-run deadline" {
  MTR_TIMEOUT_SECONDS=1
  # shellcheck disable=SC2329
  mtr() {
    while :; do :; done
  }

  execute_single_run "Standard" "ICMP4" "example.com" 1

  [ "$RUN_OK" -eq 0 ]
  [ "$RUN_FAIL" -eq 1 ]
  jq -e '._failed == true' "$JSON_LOG" >/dev/null
}
