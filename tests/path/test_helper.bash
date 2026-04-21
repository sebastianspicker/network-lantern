PROJECT_ROOT="$(cd "$(dirname "$BATS_TEST_FILENAME")/../.." && pwd)"
PATH_APP="$PROJECT_ROOT/apps/path/mtr-test-suite.sh"

assert_success() {
  [ "$status" -eq 0 ]
}

assert_failure() {
  [ "$status" -ne 0 ]
}

assert_output() {
  local mode="exact"
  local expected="${1:-}"

  if [ "${1:-}" = "--partial" ] || [ "${1:-}" = "--regexp" ]; then
    mode="$1"
    expected="${2:-}"
  fi

  case "$mode" in
    --partial)
      [[ "$output" == *"$expected"* ]]
      ;;
    --regexp)
      [[ "$output" =~ $expected ]]
      ;;
    *)
      [ "$output" = "$expected" ]
      ;;
  esac
}

assert_line() {
  local index_flag="${1:-}"
  local index_value="${2:-}"
  local expected="${3:-}"

  if [ "$index_flag" != "--index" ]; then
    return 1
  fi

  mapfile -t _assert_lines <<<"$output"
  [ "${_assert_lines[$index_value]:-__missing__}" = "$expected" ]
}
