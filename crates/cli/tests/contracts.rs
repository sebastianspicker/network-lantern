use serde_json::{Value, json};
use std::{fs, process::Command};
fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_network-lantern"))
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn every_advertised_command_has_help() {
    for args in [
        vec!["--help"],
        vec!["path", "basic", "--help"],
        vec!["path", "trace", "--help"],
        vec!["throughput", "--help"],
        vec!["workflow", "--help"],
        vec!["profiles", "--help"],
        vec!["runs", "--help"],
        vec!["tuning", "--help"],
        vec!["helper", "--help"],
    ] {
        assert!(cli(&args).status.success(), "{args:?}");
    }
}
#[test]
fn unlimited_budget_and_bidirectional_counts() {
    for (enabled, count) in [("true", 1145), ("false", 1100)] {
        let result = cli(&[
            "throughput",
            "--target",
            "does-not-exist.invalid",
            "--dry-run",
            "--max-total-tests",
            "0",
            "--bidirectional",
            enabled,
            "--json",
        ]);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stdout)
        );
        let value: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(value["plan"]["total_tests"], count);
        assert_eq!(value["plan"]["estimated_test_seconds"], count * 11);
    }
}
#[test]
fn budget_rejection_leaves_no_output() {
    let dir = tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
    let out = dir.path().join("absent");
    let result = cli(&[
        "throughput",
        "--target",
        "does-not-exist.invalid",
        "--max-total-tests",
        "1",
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(result.status.code(), Some(11));
    assert!(!out.exists());
}
#[test]
fn dry_run_never_creates_profile_or_report() {
    let dir = tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
    let out = dir.path().join("absent");
    let result = cli(&[
        "workflow",
        "baseline",
        "--target",
        "does-not-exist.invalid",
        "--dry-run",
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(!out.exists());
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["steps"][1]["plan"]["total_tests"], 1);
}
#[test]
fn legacy_profile_and_strict_unknown_keys() {
    let dir = tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
    let store = dir.path().join("profiles.json");
    fs::write(&store,serde_json::to_vec(&json!({"version":1,"profiles":{"legacy":{"Target":"fixture.invalid","SingleTest":true,"MaxTotalTests":0}}})).unwrap()).unwrap();
    let before = fs::read(&store).unwrap();
    let result = cli(&[
        "throughput",
        "--profile",
        "legacy",
        "--profiles-file",
        store.to_str().unwrap(),
        "--dry-run",
        "--json",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert_eq!(fs::read(store).unwrap(), before);
    let result = cli(&[
        "throughput",
        "--target",
        "fixture.invalid",
        "--dry-run",
        "--strict",
        "--settings",
        "{\"unknown\":1}",
        "--json",
    ]);
    assert_eq!(result.status.code(), Some(11));
}

#[test]
fn path_validation_uses_failure_status_one() {
    let result = cli(&[
        "path",
        "basic",
        "--target",
        "invalid host",
        "--dry-run",
        "--json",
    ]);
    assert_eq!(result.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["error"]["category"], "validation");
}

#[test]
fn saved_workflow_selects_identity_when_cli_name_is_omitted() {
    let dir = tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
    let store = dir.path().join("profiles.json");
    fs::write(
        &store,
        serde_json::to_vec(&json!({"version":1,"profiles":{
            "saved-baseline":{"schema_version":1,"capability":"workflow","workflow":"baseline",
            "parameters":{"throughput":{"target":"fixture.invalid"}}}
        }}))
        .unwrap(),
    )
    .unwrap();
    let result = cli(&[
        "workflow",
        "--profile",
        "saved-baseline",
        "--profiles-file",
        store.to_str().unwrap(),
        "--dry-run",
        "--json",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["workflow"], "baseline");
    assert_eq!(value["steps"][1]["plan"]["total_tests"], 1);
}

#[test]
fn parse_errors_use_the_throughput_or_general_failure_status() {
    for (args, code) in [
        (vec!["throughput", "--bogus"], 11),
        (vec!["--json", "throughput", "--bogus"], 11),
        (vec!["throughput", "--port", "not-a-port"], 11),
        (vec!["path", "basic", "--bogus"], 1),
        (vec!["workflow", "--bogus"], 1),
        (vec!["bogus"], 1),
        (vec![], 1),
    ] {
        let result = cli(&args);
        assert_eq!(result.status.code(), Some(code), "{args:?}");
        let value: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(value["error"]["category"], "validation", "{args:?}");
    }
}

#[test]
fn help_and_version_exit_zero() {
    for args in [
        vec!["--help"],
        vec!["--version"],
        vec!["throughput", "--help"],
    ] {
        assert_eq!(cli(&args).status.code(), Some(0), "{args:?}");
    }
}

#[test]
fn validation_failures_use_capability_specific_status() {
    for (args, code) in [
        (
            vec![
                "throughput",
                "--target",
                "fixture.invalid",
                "--settings",
                "[]",
                "--dry-run",
                "--json",
            ],
            11,
        ),
        (
            vec![
                "workflow",
                "baseline",
                "--settings",
                "[]",
                "--dry-run",
                "--json",
            ],
            1,
        ),
        (vec!["tuning", "--settings", "[]", "--dry-run", "--json"], 1),
        (vec!["runs", "list", "--limit", "0", "--json"], 1),
    ] {
        let result = cli(&args);
        assert_eq!(result.status.code(), Some(code), "{args:?}");
        let value: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(value["error"]["category"], "validation", "{args:?}");
    }
}
