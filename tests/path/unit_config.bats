#!/usr/bin/env bats
load test_helper

setup() {
  source "$PATH_APP"
  TEST_CONF_DIR=$(mktemp -d)
}

teardown() {
  rm -rf "$TEST_CONF_DIR"
}

@test "load_hosts_from_config parses ipv4 entries" {
  echo "ipv4=example.com" > "$TEST_CONF_DIR/hosts.conf"
  echo "ipv4=test.com" >> "$TEST_CONF_DIR/hosts.conf"
  HOSTS_IPV4=()
  HOSTS_IPV6=()
  load_hosts_from_config "$TEST_CONF_DIR/hosts.conf"
  [ ${#HOSTS_IPV4[@]} -eq 2 ]
  [ "${HOSTS_IPV4[0]}" = "example.com" ]
  [ "${HOSTS_IPV4[1]}" = "test.com" ]
}

@test "load_hosts_from_config parses ipv6 entries" {
  echo "ipv6=v6host.com" > "$TEST_CONF_DIR/hosts.conf"
  HOSTS_IPV4=(default.com)
  HOSTS_IPV6=()
  load_hosts_from_config "$TEST_CONF_DIR/hosts.conf"
  [ ${#HOSTS_IPV6[@]} -eq 1 ]
  [ "${HOSTS_IPV6[0]}" = "v6host.com" ]
  # IPv4 unchanged since no ipv4 entries
  [ "${HOSTS_IPV4[0]}" = "default.com" ]
}

@test "load_hosts_from_config skips comments and blank lines" {
  printf "# comment\n\nipv4=real.com\n" > "$TEST_CONF_DIR/hosts.conf"
  HOSTS_IPV4=()
  load_hosts_from_config "$TEST_CONF_DIR/hosts.conf"
  [ ${#HOSTS_IPV4[@]} -eq 1 ]
  [ "${HOSTS_IPV4[0]}" = "real.com" ]
}

@test "load_hosts_from_config skips lines without equals" {
  printf "invalid line\nipv4=good.com\n" > "$TEST_CONF_DIR/hosts.conf"
  HOSTS_IPV4=()
  load_hosts_from_config "$TEST_CONF_DIR/hosts.conf"
  [ ${#HOSTS_IPV4[@]} -eq 1 ]
}

@test "load_hosts_from_config missing file returns 0" {
  HOSTS_IPV4=(default.com)
  load_hosts_from_config "$TEST_CONF_DIR/nonexistent.conf"
  [ "${HOSTS_IPV4[0]}" = "default.com" ]
}

@test "load_hosts_from_config case-insensitive keys" {
  echo "IPv4=upper.com" > "$TEST_CONF_DIR/hosts.conf"
  HOSTS_IPV4=()
  load_hosts_from_config "$TEST_CONF_DIR/hosts.conf"
  [ ${#HOSTS_IPV4[@]} -eq 1 ]
}

@test "load_hosts_from_config skips empty values" {
  printf "ipv4=\nipv4=real.com\n" > "$TEST_CONF_DIR/hosts.conf"
  HOSTS_IPV4=()
  load_hosts_from_config "$TEST_CONF_DIR/hosts.conf"
  [ ${#HOSTS_IPV4[@]} -eq 1 ]
}
