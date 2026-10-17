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
    let helper_ready =
        helper["service_reachable"].as_bool() == Some(true) && helper["authorized"] == true;
    let access = lantern_path_io::native_access();
    let tuning_reason = if cfg!(windows) {
        "Windows native providers; mutations require the registered helper and a verified backup"
    } else {
        "Windows tuning requires a Windows host; plans and saved recovery records remain readable"
    };
    let mut capabilities = socket_capabilities(&access, helper_ready);
    capabilities["tuning"] = json!({"available":cfg!(windows),"reason":tuning_reason});
    json!({
        "application":"network-lantern","version":env!("CARGO_PKG_VERSION"),
        "os":std::env::consts::OS,"architecture":std::env::consts::ARCH,
        "engines":{"throughput":"native iperf3 protocol","path_basic":"native sockets","path_trace":"native sockets"},
        "helper":helper,
        "native_access":access,
        "capabilities":capabilities,
        "probes_performed":false
    })
}
/// Path and throughput readiness from the sockets this process can open (no packets sent).
fn socket_capabilities(access: &lantern_path_io::NativeAccess, helper_ready: bool) -> Value {
    let raw = access.raw_icmpv4 || access.raw_icmpv6;
    let datagram = access.datagram_icmpv4 || access.datagram_icmpv6;
    let icmp = raw || datagram || helper_ready;
    let icmp_mode = if helper_ready {
        "ICMP through the authorized helper"
    } else if raw {
        "raw ICMP sockets"
    } else if datagram {
        "unprivileged ICMP datagram sockets (no root needed)"
    } else {
        "no ICMP socket access: run with sudo, register the helper, or on Linux allow \
         unprivileged ICMP via the net.ipv4.ping_group_range sysctl"
    };
    let tcp_trace = match access.tcp_trace_mode {
        lantern_path_io::TcpTraceMode::Raw => "TCP traces send raw SYN probes",
        lantern_path_io::TcpTraceMode::Connect => {
            "TCP traces use connect(): the destination sees a completed handshake followed by RST"
        }
    };
    let mut trace_notes = vec![tcp_trace];
    if access.windows_tcp_intermediate_hops == Some(false) {
        trace_notes.push("TCP traces see only the destination without administrator rights");
    }
    if !access.traffic_class_supported {
        trace_notes.push("TOS/traffic-class rounds are not applied on this platform");
    }
    let throughput = if icmp {
        "ICMP reachability preflight available".to_string()
    } else {
        format!(
            "ICMP preflight unavailable ({icmp_mode}); live runs need sudo, the helper, or \
             {{\"skip_reachability_check\":true,\"disable_mtu_probe\":true}}"
        )
    };
    json!({
        "throughput":{"available":true,"permission":throughput},
        "path_basic":{"available":icmp,"permission":icmp_mode},
        "path_trace":{"available":icmp,"permission":icmp_mode,"notes":trace_notes},
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
    let plan = build_plan(value)?;
    log::debug!(
        "plan built: capability={} items={}",
        plan["capability"].as_str().unwrap_or("?"),
        count(&plan)
    );
    Ok(plan)
}
fn build_plan(value: &Value) -> Result<Value> {
    let req = request(value)?;
    match req.capability {
        Capability::Throughput => {
            let (plan, warnings, thresholds) = throughput(&req.layers, req.strict)?;
            Ok(
                json!({"capability":"throughput","plan":plan,"warnings":warnings,"thresholds":thresholds}),
            )
        }
        Capability::PathBasic | Capability::PathTrace => {
            let (config, mut warnings) = path_config::resolve(
                &req.layers,
                req.strict,
                req.capability == Capability::PathTrace,
            )?;
            let plan = if req.capability == Capability::PathBasic {
                serde_json::to_value(config.basic()?.0).unwrap()
            } else {
                let plan = config.trace()?.0;
                warnings.extend(plan.warnings.iter().cloned());
                serde_json::to_value(plan).unwrap()
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
            let mut warnings = plan.warnings.clone();
            for step in &plan.steps {
                let step = plan_request(&step_request(step, req.strict))?;
                for warning in step["warnings"].as_array().into_iter().flatten() {
                    if let Some(text) = warning.as_str()
                        && !warnings.iter().any(|known| known == text)
                    {
                        warnings.push(text.to_owned());
                    }
                }
                steps.push(step);
            }
            Ok(
                json!({"capability":"workflow","workflow":plan.workflow,"steps":steps,"warnings":warnings}),
            )
        }
        Capability::Tuning => {
            let (plan, mut warnings, hash) = tuning(&req.layers, req.strict)?;
            if !cfg!(windows) {
                warnings.push(format!(
                    "Windows tuning cannot run on {}; this plan is for review only",
                    std::env::consts::OS
                ));
            }
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
    fn doctor_reports_socket_access_instead_of_assuming_it() {
        let mut access = lantern_path_io::NativeAccess {
            raw_icmpv4: false,
            raw_icmpv6: false,
            datagram_icmpv4: false,
            datagram_icmpv6: false,
            raw_ip_sender: false,
            tcp_trace_mode: lantern_path_io::TcpTraceMode::Connect,
            elevated: None,
            windows_tcp_intermediate_hops: None,
            traffic_class_supported: true,
        };
        let none = socket_capabilities(&access, false);
        assert_eq!(none["path_basic"]["available"], false);
        assert!(
            none["path_basic"]["permission"]
                .as_str()
                .unwrap()
                .contains("sudo")
        );
        assert!(
            none["throughput"]["permission"]
                .as_str()
                .unwrap()
                .contains("skip_reachability_check")
        );
        assert_eq!(
            socket_capabilities(&access, true)["path_trace"]["available"],
            true
        );
        access.datagram_icmpv4 = true;
        let datagram = socket_capabilities(&access, false);
        assert_eq!(datagram["path_basic"]["available"], true);
        assert!(
            datagram["path_basic"]["permission"]
                .as_str()
                .unwrap()
                .contains("no root")
        );
    }
    #[test]
    fn tuning_plan_warns_when_not_on_windows() {
        let plan = plan_request(&json!({"capability":"tuning","layers":[{}]})).unwrap();
        let warned = plan["warnings"].as_array().unwrap().iter().any(|w| {
            w.as_str()
                .is_some_and(|w| w.contains("Windows tuning cannot run on"))
        });
        assert_eq!(warned, !cfg!(windows));
    }
    #[test]
    fn profile_capability_mismatch_is_rejected_before_planning() {
        assert!(plan_request(&json!({"capability":"path_basic","layers":[{"schema_version":1,"capability":"throughput","parameters":{}}]})).is_err());
    }
}
