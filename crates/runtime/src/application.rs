//! Request parsing, capability planning and helper status shared by CLI and desktop.
use crate::{
    config,
    errors::{helper_error, internal, throughput_error, tuning_error},
    path_config, workflow,
};
use lantern_contracts::{Error, Result};
use lantern_platform::PROFILE_LIMIT;
use lantern_throughput::{ServerCapabilities, SuiteConfig, ThroughputPlan};
use serde::Deserialize;
use serde_json::{Value, json};

/// Capabilities accepted at the request boundary; wire names are the serialized strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Capability {
    Throughput,
    PathBasic,
    PathTrace,
    Workflow,
    Tuning,
}
impl Capability {
    pub(crate) fn parse(name: &str) -> Result<Self> {
        match name {
            "throughput" => Ok(Self::Throughput),
            "path_basic" => Ok(Self::PathBasic),
            "path_trace" => Ok(Self::PathTrace),
            "workflow" => Ok(Self::Workflow),
            "tuning" => Ok(Self::Tuning),
            _ => Err(Error::validation("Unknown capability")),
        }
    }
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Throughput => "throughput",
            Self::PathBasic => "path_basic",
            Self::PathTrace => "path_trace",
            Self::Workflow => "workflow",
            Self::Tuning => "tuning",
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRequest {
    capability: String,
    #[serde(default)]
    layers: Vec<Value>,
    #[serde(default)]
    strict: bool,
    workflow: Option<workflow::Workflow>,
}
pub(crate) struct Request {
    pub capability: Capability,
    pub layers: Vec<Value>,
    pub strict: bool,
    pub workflow: Option<workflow::Workflow>,
}
pub(crate) fn request(value: &Value) -> Result<Request> {
    if serde_json::to_vec(value)
        .map_err(|e| Error::validation(e.to_string()))?
        .len()
        > PROFILE_LIMIT
    {
        return Err(Error::validation("Request exceeds 1 MiB"));
    }
    let mut request: RawRequest =
        serde_json::from_value(value.clone()).map_err(|e| Error::validation(e.to_string()))?;
    for layer in &mut request.layers {
        if layer.get("schema_version").is_some() || layer.get("capability").is_some() {
            if layer.get("schema_version").and_then(Value::as_u64) != Some(1)
                || !layer.get("parameters").is_some_and(Value::is_object)
                || layer.as_object().unwrap().keys().any(|k| {
                    !["schema_version", "capability", "parameters", "workflow"]
                        .contains(&k.as_str())
                })
            {
                return Err(Error::validation("Invalid or unsupported profile envelope"));
            }
            if layer["capability"] != request.capability {
                return Err(Error::validation(
                    "Profile capability does not match requested capability",
                ));
            }
            if request.capability == "workflow"
                && let Some(flow) = layer.get("workflow")
            {
                let saved: workflow::Workflow = serde_json::from_value(flow.clone())
                    .map_err(|e| Error::validation(e.to_string()))?;
                if request.workflow.is_some_and(|selected| selected != saved) {
                    return Err(Error::validation(
                        "Saved workflow does not match the selected workflow",
                    ));
                }
                request.workflow = Some(saved);
            }
            *layer = layer
                .get("parameters")
                .cloned()
                .ok_or_else(|| Error::validation("Profile parameters missing"))?;
        }
    }
    Ok(Request {
        capability: Capability::parse(&request.capability)?,
        layers: request.layers,
        strict: request.strict,
        workflow: request.workflow,
    })
}
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
                let normalized = workflow::workflow_layer(layer)?;
                if explicit != json!({}) {
                    workflow::deep_merge(&mut profile, &explicit);
                }
                explicit = normalized;
            }
            let plan = workflow::plan(
                req.workflow.unwrap_or_default(),
                &profile,
                &explicit,
                req.strict,
            )?;
            let mut steps = Vec::new();
            for step in &plan.steps {
                steps.push(plan_request(&workflow::step_request(step, req.strict))?);
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
