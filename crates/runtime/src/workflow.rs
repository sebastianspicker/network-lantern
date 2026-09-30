//! Pure ordered workflow resolution. Execution and filesystem paths belong to the runtime.
use crate::application::Capability;
use lantern_contracts::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Workflow {
    #[default]
    Triage,
    Path,
    Throughput,
    Baseline,
    WindowsTuning,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "capability", content = "parameters", rename_all = "snake_case")]
pub enum Step {
    Path(Value),
    Throughput(Value),
    WindowsTuning(Value),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPlan {
    pub workflow: Workflow,
    pub steps: Vec<Step>,
    pub warnings: Vec<String>,
}

pub fn plan(
    workflow: Workflow,
    profile: &Value,
    explicit: &Value,
    strict: bool,
) -> Result<WorkflowPlan> {
    let mut throughput_defaults =
        serde_json::to_value(lantern_throughput::SuiteConfig::default()).unwrap();
    throughput_defaults["target"] = Value::Null;
    throughput_defaults["max_total_tests"] = json!(0);
    throughput_defaults["bidirectional"] = json!(true);
    for key in ["min_throughput_mbps", "max_loss_pct", "max_jitter_ms"] {
        throughput_defaults[key] = Value::Null;
    }
    let defaults = json!({
        "path":crate::path_config::PathConfig::default(),
        "throughput":throughput_defaults,
        "windowsTuning":lantern_tuning::TuningConfig::default()
    });
    let mut resolved = defaults.clone();
    let mut warnings = Vec::new();
    for layer in [profile, explicit] {
        if layer.is_null() {
            continue;
        }
        let object = layer
            .as_object()
            .ok_or_else(|| Error::validation("Workflow configuration must be an object"))?;
        for (name, values) in object {
            let canonical = defaults
                .as_object()
                .unwrap()
                .keys()
                .find(|key| key.eq_ignore_ascii_case(name));
            let Some(canonical) = canonical else {
                warn(
                    &mut warnings,
                    strict,
                    format!("Unknown workflow profile section '{name}' ignored"),
                )?;
                continue;
            };
            if !values.is_object() {
                warn(
                    &mut warnings,
                    strict,
                    format!("Workflow section '{name}' is not an object and was ignored"),
                )?;
                continue;
            }
            let translated = match canonical.as_str() {
                "path" => crate::path_config::translate(values)?,
                "throughput" => crate::config::legacy_throughput(values)?,
                _ => values.clone(),
            };
            let (value, notes) = crate::config::merge_layers(
                resolved[canonical].clone(),
                std::slice::from_ref(&translated),
                strict,
            )?;
            resolved[canonical] = value;
            warnings.extend(notes.into_iter().map(|note| format!("{name}: {note}")));
        }
    }
    let path = &resolved["path"];
    let throughput = &resolved["throughput"];
    let tuning = &resolved["windowsTuning"];
    for key in ["hosts_ipv4", "hosts_ipv6", "protocols", "rounds"] {
        strings(&path[key], &format!("path.{key}"))?;
    }
    for protocol in strings(&path["protocols"], "path.protocols")? {
        if !["ipv4", "ipv6"].contains(&protocol.to_ascii_lowercase().as_str()) {
            return Err(Error::validation("path.protocols supports IPv4 and IPv6"));
        }
    }
    integer(&throughput["port"], "throughput.port", 1, 65535)?;
    integer(
        &throughput["max_total_tests"],
        "throughput.maxTotalTests",
        0,
        1_000_000,
    )?;
    choice(
        &throughput["protocol"],
        "throughput.protocol",
        &["tcp", "udp", "both"],
    )?;
    choice(
        &tuning["action"],
        "windowsTuning.action",
        &["apply", "backup", "restore", "verify"],
    )?;
    choice(
        &tuning["profile"],
        "windowsTuning.profile",
        &["safe", "measured"],
    )?;
    strings(&tuning["appPaths"], "windowsTuning.appPaths")?;
    for port in tuning["udpPorts"]
        .as_array()
        .ok_or_else(|| Error::validation("udpPorts must be an array"))?
    {
        integer(port, "udpPorts", 0, 65535)?;
    }
    if !throughput["target"].is_null() && !throughput["target"].is_string() {
        return Err(Error::validation("throughput.target must be a string"));
    }
    crate::path_config::resolve(std::slice::from_ref(path), true, false)?
        .0
        .basic()?;
    if !tuning["includeAppPolicies"].is_boolean() {
        return Err(Error::validation(
            "windowsTuning.includeAppPolicies must be true or false",
        ));
    }
    let mut checked_throughput = throughput.clone();
    if checked_throughput["target"].is_null() {
        checked_throughput["target"] = json!("profile-validation.invalid");
    }
    checked_throughput["max_total_tests"] = json!(0);
    crate::plan_request(
        &json!({"capability":"throughput","layers":[checked_throughput],"strict":true}),
    )?;
    let has_target = throughput["target"]
        .as_str()
        .is_some_and(|s| !s.trim().is_empty());
    if matches!(workflow, Workflow::Throughput | Workflow::Baseline) && !has_target {
        return Err(Error::validation("A throughput target is required"));
    }
    let mut steps = Vec::new();
    if matches!(
        workflow,
        Workflow::Triage | Workflow::Path | Workflow::Baseline
    ) {
        steps.push(Step::Path(path.clone()));
    }
    if matches!(workflow, Workflow::Throughput | Workflow::Baseline)
        || workflow == Workflow::Triage && has_target
    {
        let mut parameters = throughput.clone();
        if workflow == Workflow::Baseline {
            parameters["single_test"] = json!(true);
        }
        steps.push(Step::Throughput(parameters));
    } else if workflow == Workflow::Triage {
        warnings.push("Throughput skipped: no target provided for Triage workflow".into());
    }
    if workflow == Workflow::WindowsTuning {
        steps.push(Step::WindowsTuning(tuning.clone()));
    }
    Ok(WorkflowPlan {
        workflow,
        steps,
        warnings,
    })
}
fn warn(warnings: &mut Vec<String>, strict: bool, message: String) -> Result<()> {
    if strict {
        Err(Error::validation(message))
    } else {
        warnings.push(message);
        Ok(())
    }
}
pub fn strings(value: &Value, name: &str) -> Result<Vec<String>> {
    if let Some(text) = value.as_str() {
        return Ok(vec![text.into()]);
    }
    value
        .as_array()
        .ok_or_else(|| Error::validation(format!("{name} must be a string or array")))?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| Error::validation(format!("{name} must contain strings")))
        })
        .collect()
}
fn integer(value: &Value, name: &str, min: u64, max: u64) -> Result<u64> {
    value
        .as_u64()
        .filter(|n| *n >= min && *n <= max)
        .ok_or_else(|| {
            Error::validation(format!("{name} must be an integer between {min} and {max}"))
        })
}
fn choice(value: &Value, name: &str, allowed: &[&str]) -> Result<()> {
    if value
        .as_str()
        .is_some_and(|s| allowed.contains(&s.to_ascii_lowercase().as_str()))
    {
        Ok(())
    } else {
        Err(Error::validation(format!("Invalid {name}")))
    }
}
/// Merges nested objects from `source` into `target`; other values replace.
pub(crate) fn deep_merge(target: &mut Value, source: &Value) {
    if let (Some(target), Some(source)) = (target.as_object_mut(), source.as_object()) {
        for (k, v) in source {
            if v.is_object() {
                let entry = target.entry(k).or_insert_with(|| json!({}));
                deep_merge(entry, v);
            } else {
                target.insert(k.clone(), v.clone());
            }
        }
    }
}
/// Moves flat throughput overrides into the workflow's `throughput` section.
pub(crate) fn workflow_layer(layer: &Value) -> Result<Value> {
    let mut value = layer.clone();
    let object = value
        .as_object_mut()
        .ok_or_else(|| Error::validation("Workflow settings must be an object"))?;
    for (from, to) in [
        ("target", "target"),
        ("port", "port"),
        ("protocol", "protocol"),
        ("max_total_tests", "maxTotalTests"),
        ("duration_secs", "duration_secs"),
        ("omit_secs", "omit_secs"),
        ("single_test", "single_test"),
        ("bidirectional", "bidirectional"),
    ] {
        if let Some(v) = object.remove(from) {
            let throughput = object
                .entry("throughput")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or_else(|| {
                    Error::validation(
                        "Workflow section 'throughput' must be an object when applying overrides",
                    )
                })?;
            throughput.insert(to.into(), v);
        }
    }
    Ok(value)
}
/// The capability request that plans one resolved workflow step.
pub(crate) fn step_request(step: &Step, strict: bool) -> Value {
    let (capability, layer) = match step {
        Step::Path(v) => (Capability::PathBasic, v),
        Step::Throughput(v) => (Capability::Throughput, v),
        Step::WindowsTuning(v) => (Capability::Tuning, v),
    };
    json!({"capability":capability.as_str(),"layers":[layer],"strict":strict})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn baseline_order_and_single_test() {
        let p = plan(
            Workflow::Baseline,
            &json!({"throughput":{"target":"unresolvable.invalid"}}),
            &json!({}),
            false,
        )
        .unwrap();
        assert!(matches!(p.steps[0], Step::Path(_)));
        assert!(matches!(&p.steps[1],Step::Throughput(v) if v["single_test"]==true));
    }
    #[test]
    fn triage_without_target_warns() {
        let p = plan(Workflow::Triage, &json!({}), &json!({}), false).unwrap();
        assert_eq!(p.steps.len(), 1);
        assert_eq!(p.warnings.len(), 1);
    }
    #[test]
    fn explicit_zero_and_repeated_hosts_preserved() {
        let p = plan(
            Workflow::Triage,
            &json!({"throughput":{"target":"fixture","maxTotalTests":5}}),
            &json!({"throughput":{"maxTotalTests":0},"path":{"hostsIPv4":["fixture","fixture"]}}),
            false,
        )
        .unwrap();
        assert!(matches!(&p.steps[1],Step::Throughput(v) if v["max_total_tests"]==0));
        assert!(
            matches!(&p.steps[0],Step::Path(v) if v["hosts_ipv4"].as_array().unwrap().len()==2)
        );
    }
    #[test]
    fn validate_irrelevant_profile_sections() {
        assert!(
            plan(
                Workflow::Path,
                &json!({"throughput":{"port":false}}),
                &json!({}),
                false
            )
            .is_err()
        );
    }
}
