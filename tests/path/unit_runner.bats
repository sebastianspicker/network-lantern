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
}

teardown() {
  rm -rf "$TEST_RUN_DIR"
}

@test "failed mtr output keeps JSON log parseable" {
  mtr() {
    printf 'not json\nsecond line\n'
    return 1
  }

  execute_single_run "Standard" "ICMP4" "example.com" 1

  [ "$RUN_FAIL" -eq 1 ]
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    python3 -c 'import json,sys; json.loads(sys.argv[1])' "$line"
  done <"$JSON_LOG"
}
