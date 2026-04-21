#!/usr/bin/env bats
load test_helper

setup() {
  source "$PATH_APP"
}

# set_round_extra_args tests
@test "set_round_extra_args Standard sets empty extra_args" {
  set_round_extra_args Standard
  [ ${#extra_args[@]} -eq 0 ]
}

@test "set_round_extra_args MTU1400 sets -s 1400" {
  set_round_extra_args MTU1400
  [[ "${extra_args[*]}" == "-s 1400" ]]
}

@test "set_round_extra_args TOS_CS5 sets --tos 160" {
  set_round_extra_args TOS_CS5
  [[ "${extra_args[*]}" == "--tos 160" ]]
}

@test "set_round_extra_args TOS_AF11 sets --tos 40" {
  set_round_extra_args TOS_AF11
  [[ "${extra_args[*]}" == "--tos 40" ]]
}

@test "set_round_extra_args TTL10 sets -m 10" {
  set_round_extra_args TTL10
  [[ "${extra_args[*]}" == "-m 10" ]]
}

@test "set_round_extra_args TTL64 sets -m 64" {
  set_round_extra_args TTL64
  [[ "${extra_args[*]}" == "-m 64" ]]
}

@test "set_round_extra_args FirstTTL3 sets -f 3" {
  set_round_extra_args FirstTTL3
  [[ "${extra_args[*]}" == "-f 3" ]]
}

@test "set_round_extra_args Timeout5 sets -Z 5" {
  set_round_extra_args Timeout5
  [[ "${extra_args[*]}" == "-Z 5" ]]
}

@test "set_round_extra_args unknown round dies" {
  run set_round_extra_args BOGUS
  assert_failure
}

# set_mtr_args_for_type tests
@test "set_mtr_args_for_type ICMP4 includes -4" {
  set_mtr_args_for_type ICMP4
  [[ "${mtr_args[*]}" == *"-4"* ]]
}

@test "set_mtr_args_for_type ICMP6 includes -6" {
  set_mtr_args_for_type ICMP6
  [[ "${mtr_args[*]}" == *"-6"* ]]
}

@test "set_mtr_args_for_type UDP4 includes -u -4" {
  set_mtr_args_for_type UDP4
  [[ "${mtr_args[*]}" == *"-u"* ]] && [[ "${mtr_args[*]}" == *"-4"* ]]
}

@test "set_mtr_args_for_type TCP4 includes -T -P 443 -4" {
  set_mtr_args_for_type TCP4
  [[ "${mtr_args[*]}" == *"-T"* ]] && [[ "${mtr_args[*]}" == *"-P"* ]] && [[ "${mtr_args[*]}" == *"443"* ]]
}

@test "set_mtr_args_for_type MPLS4 includes -e -4" {
  set_mtr_args_for_type MPLS4
  [[ "${mtr_args[*]}" == *"-e"* ]] && [[ "${mtr_args[*]}" == *"-4"* ]]
}

@test "set_mtr_args_for_type AS6 includes -z --aslookup -6" {
  set_mtr_args_for_type AS6
  [[ "${mtr_args[*]}" == *"-z"* ]] && [[ "${mtr_args[*]}" == *"--aslookup"* ]] && [[ "${mtr_args[*]}" == *"-6"* ]]
}

@test "set_mtr_args_for_type UNKNOWN dies" {
  run set_mtr_args_for_type BOGUS
  assert_failure
  assert_output --partial "Unknown test type"
}

@test "set_mtr_args_for_type all types include --json" {
  local t
  for t in ICMP4 ICMP6 UDP4 UDP6 TCP4 TCP6 MPLS4 MPLS6 AS4 AS6; do
    set_mtr_args_for_type "$t"
    [[ "${mtr_args[*]}" == *"--json"* ]]
  done
}

# hosts_for_type tests
@test "hosts_for_type IPv4 type returns HOSTS_IPV4" {
  HOSTS_IPV4=(a.com b.com)
  HOSTS_IPV6=(c.com)
  run hosts_for_type ICMP4
  assert_success
  assert_line --index 0 "a.com"
  assert_line --index 1 "b.com"
}

@test "hosts_for_type IPv6 type returns HOSTS_IPV6" {
  HOSTS_IPV4=(a.com)
  HOSTS_IPV6=(c.com d.com)
  run hosts_for_type ICMP6
  assert_success
  assert_line --index 0 "c.com"
  assert_line --index 1 "d.com"
}

@test "hosts_for_type empty IPv6 returns nothing" {
  HOSTS_IPV4=(a.com)
  HOSTS_IPV6=()
  run hosts_for_type ICMP6
  assert_success
  assert_output ""
}
