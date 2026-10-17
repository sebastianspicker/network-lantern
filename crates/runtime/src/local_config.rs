//! Read legacy host defaults once, before creating a reviewable pure request.
use lantern_contracts::{Error, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Environment variable naming the hosts file; it overrides `config/hosts.conf` in the CWD.
pub const HOSTS_FILE_ENV: &str = "NETWORK_LANTERN_HOSTS_FILE";

/// Returns the request with host defaults layered in, plus warnings for the plan preview.
pub fn prepare_local_request(request: &Value) -> Result<(Value, Vec<String>)> {
    let explicit = std::env::var_os(HOSTS_FILE_ENV)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    with_hosts_file(request, explicit.as_deref(), Path::new("config/hosts.conf"))
}
/// Appends warnings from [`prepare_local_request`] to the plan's `warnings` list.
pub fn add_plan_warnings(plan: &mut Value, warnings: Vec<String>) {
    if warnings.is_empty() {
        return;
    }
    if let Some(list) = plan.get_mut("warnings").and_then(Value::as_array_mut) {
        list.extend(warnings.into_iter().map(Value::from));
    } else {
        plan["warnings"] = json!(warnings);
    }
}
fn has_explicit_hosts(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            (matches!(
                key.to_ascii_lowercase().as_str(),
                "hosts_ipv4" | "hosts_ipv6" | "hostsipv4" | "hostsipv6"
            ) && value.as_array().is_some_and(|hosts| !hosts.is_empty()))
                || has_explicit_hosts(value)
        }),
        Value::Array(items) => items.iter().any(has_explicit_hosts),
        _ => false,
    }
}
/// Path capabilities and the workflows with a path step probe hosts; the others never do.
fn uses_hosts(request: &Value) -> bool {
    match request.get("capability").and_then(Value::as_str) {
        Some("workflow") => matches!(
            request.get("workflow").and_then(Value::as_str),
            Some("triage" | "path" | "baseline")
        ),
        _ => true,
    }
}
/// Strips a trailing `# comment`; `#` only starts one at line start or after whitespace.
fn strip_comment(line: &str) -> &str {
    let mut previous_space = true;
    for (index, c) in line.char_indices() {
        if c == '#' && previous_space {
            return &line[..index];
        }
        previous_space = c.is_whitespace();
    }
    line
}
fn with_hosts_file(
    request: &Value,
    explicit: Option<&Path>,
    default: &Path,
) -> Result<(Value, Vec<String>)> {
    let capability = request.get("capability").and_then(Value::as_str);
    if !matches!(capability, Some("path_basic" | "path_trace" | "workflow")) {
        return Ok((request.clone(), Vec::new()));
    }
    let path = explicit.unwrap_or(default);
    if !path
        .try_exists()
        .map_err(|e| Error::validation(e.to_string()))?
    {
        if explicit.is_some() {
            return Err(Error::validation(format!(
                "{HOSTS_FILE_ENV} points to '{}', which does not exist",
                path.display()
            )));
        }
        let mut warnings = Vec::new();
        if uses_hosts(request) && !request.get("layers").is_some_and(has_explicit_hosts) {
            let cwd = std::env::current_dir()
                .map(|dir| dir.display().to_string())
                .unwrap_or_else(|_| "the working directory".into());
            warnings.push(format!(
                "No config/hosts.conf in {cwd}; using built-in default hosts"
            ));
        }
        return Ok((request.clone(), warnings));
    }
    let bytes = lantern_platform::read_bounded(path, 64 * 1024)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| Error::validation("hosts.conf must contain UTF-8 text"))?;
    let mut hosts4 = Vec::new();
    let mut hosts6 = Vec::new();
    for line in text.lines().map(|line| strip_comment(line).trim()) {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.trim().to_ascii_lowercase().as_str() {
            "ipv4" => hosts4.push(value),
            "ipv6" => hosts6.push(value),
            _ => {}
        }
    }
    let mut warnings = Vec::new();
    if hosts4.is_empty()
        && hosts6.is_empty()
        && uses_hosts(request)
        && !request.get("layers").is_some_and(has_explicit_hosts)
    {
        warnings.push(format!(
            "{} lists no ipv4=/ipv6= hosts; using built-in default hosts",
            path.display()
        ));
    }
    let mut hosts = serde_json::Map::new();
    if !hosts4.is_empty() {
        hosts.insert("hosts_ipv4".into(), json!(hosts4));
    }
    if !hosts6.is_empty() {
        hosts.insert("hosts_ipv6".into(), json!(hosts6));
    }
    let layer = if capability == Some("workflow") {
        json!({"path":hosts})
    } else {
        Value::Object(hosts)
    };
    let mut prepared = request.clone();
    prepared
        .get_mut("layers")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| Error::validation("Request layers must be an array"))?
        .insert(0, layer);
    Ok((prepared, warnings))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_defaults_are_snapshotted_and_explicit_hosts_win() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().canonicalize().unwrap().join("hosts.conf");
        std::fs::write(&path, "# comment\n IPV4 = first.invalid\nipv4=first.invalid\ninvalid\nipv6=third.invalid\nunknown=ignored\n").unwrap();
        let request = json!({"capability":"path_basic","layers":[{"hosts_ipv6":["explicit.invalid"],"rounds":["Standard"]}]});
        let (prepared, warnings) = with_hosts_file(&request, None, &path).unwrap();
        assert!(warnings.is_empty());
        std::fs::write(&path, "ipv4=changed.invalid").unwrap();
        let plan = crate::plan_request(&prepared).unwrap();
        assert_eq!(
            plan["plan"]["ipv4_hosts"],
            json!(["first.invalid", "first.invalid"])
        );
        assert_eq!(plan["plan"]["ipv6_hosts"], json!(["explicit.invalid"]));
        assert_eq!(plan["plan"]["total_items"], 3);
    }
    #[test]
    fn absent_or_empty_legacy_config_preserves_fallbacks() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().canonicalize().unwrap().join("hosts.conf");
        let request = json!({"capability":"path_trace","layers":[]});
        let (prepared, warnings) = with_hosts_file(&request, None, &path).unwrap();
        assert_eq!(prepared, request);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("No config/hosts.conf in"));
        assert!(warnings[0].contains("using built-in default hosts"));
        let explicit = json!({"capability":"path_trace","layers":[{"hosts_ipv4":["a.invalid"]}]});
        assert!(
            with_hosts_file(&explicit, None, &path)
                .unwrap()
                .1
                .is_empty()
        );
        std::fs::write(&path, "# empty\nunknown=ignored").unwrap();
        assert_eq!(
            crate::plan_request(&with_hosts_file(&request, None, &path).unwrap().0).unwrap(),
            crate::plan_request(&request).unwrap()
        );
    }
    #[test]
    fn explicit_hosts_file_overrides_default_and_must_exist() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let chosen = root.join("chosen.conf");
        std::fs::write(&chosen, "ipv4=env.invalid").unwrap();
        let request = json!({"capability":"path_basic","layers":[]});
        let missing_default = root.join("missing.conf");
        let (prepared, warnings) =
            with_hosts_file(&request, Some(&chosen), &missing_default).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(prepared["layers"][0]["hosts_ipv4"], json!(["env.invalid"]));
        let absent = root.join("absent.conf");
        let error = with_hosts_file(&request, Some(&absent), &missing_default).unwrap_err();
        assert_eq!(error.category, lantern_contracts::ErrorCategory::Validation);
        assert!(error.message.contains("absent.conf"));
    }
    #[test]
    fn inline_comments_are_not_part_of_hosts() {
        assert_eq!(strip_comment("ipv4=a.example # note"), "ipv4=a.example ");
        assert_eq!(strip_comment("# whole line"), "");
        assert_eq!(strip_comment("ipv4=a#b.example"), "ipv4=a#b.example");
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().canonicalize().unwrap().join("hosts.conf");
        std::fs::write(&path, "ipv4=a.example # note\nipv6=b.example\t#x\n").unwrap();
        let request = json!({"capability":"path_basic","layers":[]});
        let (prepared, _) = with_hosts_file(&request, None, &path).unwrap();
        assert_eq!(prepared["layers"][0]["hosts_ipv4"], json!(["a.example"]));
        assert_eq!(prepared["layers"][0]["hosts_ipv6"], json!(["b.example"]));
    }
    #[test]
    fn plan_warnings_are_appended() {
        let mut plan = json!({"warnings":["old"]});
        add_plan_warnings(&mut plan, vec!["new".into()]);
        assert_eq!(plan["warnings"], json!(["old", "new"]));
        let mut bare = json!({});
        add_plan_warnings(&mut bare, vec!["new".into()]);
        assert_eq!(bare["warnings"], json!(["new"]));
    }
}
