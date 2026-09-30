//! Request parsing, capability planning and helper status shared by CLI and desktop.
use crate::{
    config,
    errors::{helper_error, internal, throughput_error, tuning_error},
    path_config,
    request::{Capability, request},
    workflow::{self, Step, Workflow, WorkflowPlan},
};
use lantern_contracts::{Error, Result};
use lantern_throughput::{ServerCapabilities, SuiteConfig, ThroughputPlan};
use serde_json::{Value, json};

pub async fn doctor() -> Value {
    let helper = helper_operation("status")
        .await
        .unwrap_or_else(|e| json!({"error":e}));
    let tuning_reason = if cfg!(windows) {
        "Windows native providers; mutations require the registered helper and a verified backup"
    } else {
        "Windows tuning requires a Windows host; plans and saved recovery records remain readable"
    };
    json!({
        "application":"network-lantern","version":env!("CARGO_PKG_VERSION"),
        "os":std::env::consts::OS,"architecture":std::env::consts::ARCH,
        "engines":{"throughput":"native iperf3 protocol","path_basic":"native sockets","path_trace":"native sockets"},
        "helper":helper,
        "capabilities":{
            "throughput":{"available":true,
                "permission":"Native ICMP preflight may require helper authorization; explicit skip controls support TCP-only networks"},
            "path_basic":{"available":true,"permission":"Native ICMP sockets; platform authorization may be required"},
            "path_trace":{"available":true,
                "permission":"Raw ICMP/UDP/TCP requires platform helper authorization where sockets are restricted"},
            "tuning":{"available":cfg!(windows),"reason":tuning_reason}
        },
        "probes_performed":false
    })
}
pub async fn helper_operation(operation: &str) -> Result<Value> {
    let client = lantern_helper::HelperClient::new();
    let status = match operation {
        "status" => client.status().await,
        "register" => client.register().await,
        "remove" => client.remove().await,
        _ => return Err(Error::validation("Unknown helper operation")),
    }
    .map_err(helper_error)?;
    serde_json::to_value(status).map_err(internal)
}
pub(crate) fn tuning(
    layers: &[Value],
    strict: bool,
) -> Result<(lantern_tuning::TuningPlan, Vec<String>, String)> {
    let (value, warnings) = config::merge_layers(
        serde_json::to_value(lantern_tuning::TuningConfig::default()).unwrap(),
        layers,
        strict,
    )?;
    let config = serde_json::from_value(value).map_err(|e| Error::validation(e.to_string()))?;
    let plan = lantern_tuning::plan(&config).map_err(tuning_error)?;
    let hash = lantern_helper::HelperClient::reviewed_hash(
        &lantern_helper::HelperOperation::ExecuteTuning { plan: plan.clone() },
    )
    .map_err(helper_error)?;
    Ok((plan, warnings, hash))
}
pub(crate) fn throughput(
    layers: &[Value],
    strict: bool,
) -> Result<(ThroughputPlan, Vec<String>, crate::thresholds::Thresholds)> {
    let mut defaults = serde_json::to_value(SuiteConfig::default()).unwrap();
    defaults["bidirectional"] = json!(true);
    defaults["min_throughput_mbps"] = Value::Null;
    defaults["max_loss_pct"] = Value::Null;
    defaults["max_jitter_ms"] = Value::Null;
    let translated = layers
        .iter()
        .map(config::legacy_throughput)
        .collect::<Result<Vec<_>>>()?;
    let (mut value, warnings) = config::merge_layers(defaults, &translated, strict)?;
    let bidirectional = value
        .as_object_mut()
        .unwrap()
        .remove("bidirectional")
        .unwrap();
    let bidirectional = bidirectional
        .as_bool()
        .ok_or_else(|| Error::validation("bidirectional must be true or false"))?;
    if value["max_total_tests"] == 0 {
        value["max_total_tests"] = Value::Null;
    }
    let thresholds = crate::thresholds::Thresholds {
        min_throughput_mbps: serde_json::from_value(
            value
                .as_object_mut()
                .unwrap()
                .remove("min_throughput_mbps")
                .unwrap(),
        )
        .map_err(|e| Error::validation(e.to_string()))?,
        max_loss_pct: serde_json::from_value(
            value
                .as_object_mut()
                .unwrap()
                .remove("max_loss_pct")
                .unwrap(),
        )
        .map_err(|e| Error::validation(e.to_string()))?,
        max_jitter_ms: serde_json::from_value(
            value
                .as_object_mut()
                .unwrap()
                .remove("max_jitter_ms")
                .unwrap(),
        )
        .map_err(|e| Error::validation(e.to_string()))?,
    };
    thresholds.validate()?;
    let config = serde_json::from_value(value).map_err(|e| Error::validation(e.to_string()))?;
    let plan = lantern_throughput::plan(&config, ServerCapabilities { bidirectional })
        .map_err(throughput_error)?;
    Ok((plan, warnings, thresholds))
}
pub fn plan_request(value: &Value) -> Result<Value> {
    let req = request(value)?;
    match req.capability {
        Capability::Throughput => {
            let (plan, warnings, thresholds) = throughput(&req.layers, req.strict)?;
            Ok(
                json!({"capability":"throughput","plan":plan,"warnings":warnings,"thresholds":thresholds}),
            )
        }
        Capability::PathBasic | Capability::PathTrace => {
            let (config, warnings) = path_config::resolve(
                &req.layers,
                req.strict,
                req.capability == Capability::PathTrace,
            )?;
            let plan = if req.capability == Capability::PathBasic {
                serde_json::to_value(config.basic()?.0).unwrap()
            } else {
                serde_json::to_value(config.trace()?.0).unwrap()
            };
            Ok(json!({
                "capability":req.capability.as_str(),"plan":plan,"settings":config,"warnings":warnings,
                "asn_disclosure":"AS4/AS6 sends hop IP addresses to Team Cymru DNS only when selected"
            }))
        }
        Capability::Workflow => {
            let mut profile = json!({});
            let mut explicit = json!({});
            for layer in &req.layers {
                let normalized = workflow_layer(layer)?;
                if explicit != json!({}) {
                    deep_merge(&mut profile, &explicit);
                }
                explicit = normalized;
            }
            let plan = resolve_workflow(
                req.workflow.unwrap_or_default(),
                &profile,
                &explicit,
                req.strict,
            )?;
            let mut steps = Vec::new();
            for step in &plan.steps {
                steps.push(plan_request(&step_request(step, req.strict))?);
            }
            Ok(
                json!({"capability":"workflow","workflow":plan.workflow,"steps":steps,"warnings":plan.warnings}),
            )
        }
        Capability::Tuning => {
            let (plan, warnings, hash) = tuning(&req.layers, req.strict)?;
            Ok(json!({
                "capability":"tuning","settings":plan.config,"plan":plan,"warnings":warnings,
                "authorization":{"reviewed_operation_hash":hash},"total_items":1,"platform":"windows"
            }))
        }
    }
}
/// Resolves a workflow, validating its throughput section as a strict throughput request.
pub(crate) fn resolve_workflow(
    workflow: Workflow,
    profile: &Value,
    explicit: &Value,
    strict: bool,
) -> Result<WorkflowPlan> {
    workflow::plan(
        workflow,
        profile,
        explicit,
        strict,
        check_workflow_throughput,
    )
}
pub(crate) fn check_workflow_throughput(layer: &Value) -> Result<()> {
    plan_request(&json!({"capability":"throughput","layers":[layer],"strict":true})).map(|_| ())
}
/// Merges nested objects from `source` into `target`; other values replace.
fn deep_merge(target: &mut Value, source: &Value) {
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
fn workflow_layer(layer: &Value) -> Result<Value> {
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
fn step_request(step: &Step, strict: bool) -> Value {
    let (capability, layer) = match step {
        Step::Path(v) => (Capability::PathBasic, v),
        Step::Throughput(v) => (Capability::Throughput, v),
        Step::WindowsTuning(v) => (Capability::Tuning, v),
    };
    json!({"capability":capability.as_str(),"layers":[layer],"strict":strict})
}
pub(crate) fn count(preview: &Value) -> u64 {
    preview["plan"]["total_tests"]
        .as_u64()
        .or_else(|| preview["plan"]["total_items"].as_u64())
        .or_else(|| preview["total_items"].as_u64())
        .unwrap_or_else(|| {
            preview["steps"]
                .as_array()
                .map(|v| v.iter().map(count).sum())
                .unwrap_or(0)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_workflow_throughput_with_flat_overrides_returns_validation() {
        for section in [
            Value::Null,
            json!(true),
            json!(42),
            json!("invalid"),
            json!([]),
        ] {
            for overrides in [json!({"target":"fixture.invalid"}), json!({"port":5003})] {
                for strict in [false, true] {
                    let mut layer = overrides.clone();
                    layer["throughput"] = section.clone();
                    let error = plan_request(&json!({
                        "capability":"workflow","layers":[layer],"strict":strict
                    }))
                    .unwrap_err();
                    assert_eq!(error.category, lantern_contracts::ErrorCategory::Validation);
                    assert!(error.message.contains("throughput"));
                }
            }
        }
    }
    #[test]
    fn profile_capability_mismatch_is_rejected_before_planning() {
        assert!(plan_request(&json!({"capability":"path_basic","layers":[{"schema_version":1,"capability":"throughput","parameters":{}}]})).is_err());
    }
}
