use crate::{config, manager::RunManager, path_config, workflow};
use lantern_contracts::{Error, ErrorCategory, Provenance, RECORD_VERSION, Result, now};
use lantern_platform::{PROFILE_LIMIT, REPORT_LIMIT, atomic_json, read_json};
use lantern_throughput::{ServerCapabilities, SuiteConfig, ThroughputError, ThroughputPlan};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    capability: String,
    #[serde(default)]
    layers: Vec<Value>,
    #[serde(default)]
    strict: bool,
    workflow: Option<workflow::Workflow>,
}
fn request(value: &Value) -> Result<Request> {
    if serde_json::to_vec(value)
        .map_err(|e| Error::validation(e.to_string()))?
        .len()
        > PROFILE_LIMIT
    {
        return Err(Error::validation("Request exceeds 1 MiB"));
    }
    let mut request: Request =
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
    Ok(request)
}
pub async fn doctor() -> Value {
    let helper = helper_operation("status")
        .await
        .unwrap_or_else(|e| json!({"error":e}));
    json!({"application":"network-lantern","version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,"engines":{"throughput":"native iperf3 protocol","path_basic":"native sockets","path_trace":"native sockets"},"helper":helper,"capabilities":{
        "throughput":{"available":true,"permission":"Native ICMP preflight may require helper authorization; explicit skip controls support TCP-only networks"},
        "path_basic":{"available":true,"permission":"Native ICMP sockets; platform authorization may be required"},
        "path_trace":{"available":true,"permission":"Raw ICMP/UDP/TCP requires platform helper authorization where sockets are restricted"},
        "tuning":{"available":cfg!(windows),"reason":if cfg!(windows){"Windows native providers; mutations require the registered helper and a verified backup"}else{"Windows tuning requires a Windows host; plans and saved recovery records remain readable"}}
    },"probes_performed":false})
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
    serde_json::to_value(status).map_err(|e| Error::new(ErrorCategory::Internal, e.to_string()))
}
pub(crate) fn helper_error(error: lantern_helper::HelperError) -> Error {
    use lantern_helper::HelperError as H;
    let category = match &error {
        H::Validation(_) => ErrorCategory::Validation,
        H::Authorization(_) | H::Authentication | H::Replay | H::Scope(_) => {
            ErrorCategory::Permission
        }
        H::Cancelled => ErrorCategory::Cancelled,
        H::Diagnostic(error) => error.category,
        H::Probe(_) => ErrorCategory::Connectivity,
        _ => ErrorCategory::Prerequisite,
    };
    Error::new(category, error.to_string())
}
fn tuning_error(error: lantern_tuning::TuningError) -> Error {
    use lantern_tuning::TuningError as T;
    let category = match &error {
        T::Validation(_) => ErrorCategory::Validation,
        T::Authorization(_) => ErrorCategory::Permission,
        T::Cancelled => ErrorCategory::Cancelled,
        _ => ErrorCategory::Prerequisite,
    };
    Error::new(category, error.to_string())
}
fn tuning(
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
fn throughput(
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
    match req.capability.as_str() {
        "throughput" => {
            let (plan, warnings, thresholds) = throughput(&req.layers, req.strict)?;
            Ok(
                json!({"capability":"throughput","plan":plan,"warnings":warnings,"thresholds":thresholds}),
            )
        }
        "path_basic" | "path_trace" => {
            let (config, warnings) =
                path_config::resolve(&req.layers, req.strict, req.capability == "path_trace")?;
            let plan = if req.capability == "path_basic" {
                serde_json::to_value(config.basic()?.0).unwrap()
            } else {
                serde_json::to_value(config.trace()?.0).unwrap()
            };
            Ok(
                json!({"capability":req.capability,"plan":plan,"settings":config,"warnings":warnings,"asn_disclosure":"AS4/AS6 sends hop IP addresses to Team Cymru DNS only when selected"}),
            )
        }
        "workflow" => {
            let mut profile = json!({});
            let mut explicit = json!({});
            for layer in &req.layers {
                let normalized = workflow_layer(layer)?;
                if explicit != json!({}) {
                    deep_merge(&mut profile, &explicit);
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
                steps.push(plan_request(&step_request(step, req.strict))?);
            }
            Ok(
                json!({"capability":"workflow","workflow":plan.workflow,"steps":steps,"warnings":plan.warnings}),
            )
        }
        "tuning" => {
            let (plan, warnings, hash) = tuning(&req.layers, req.strict)?;
            Ok(
                json!({"capability":"tuning","settings":plan.config,"plan":plan,"warnings":warnings,"authorization":{"reviewed_operation_hash":hash},"total_items":1,"platform":"windows"}),
            )
        }
        _ => Err(Error::validation("Unknown capability")),
    }
}
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
fn step_request(step: &workflow::Step, strict: bool) -> Value {
    match step {
        workflow::Step::Path(v) => json!({"capability":"path_basic","layers":[v],"strict":strict}),
        workflow::Step::Throughput(v) => {
            json!({"capability":"throughput","layers":[v],"strict":strict})
        }
        workflow::Step::WindowsTuning(v) => {
            json!({"capability":"tuning","layers":[v],"strict":strict})
        }
    }
}
pub(crate) fn throughput_error(e: ThroughputError) -> Error {
    let category = match &e {
        ThroughputError::Validation(_) | ThroughputError::BudgetExceeded { .. } => {
            ErrorCategory::Validation
        }
        ThroughputError::Cancelled => ErrorCategory::Cancelled,
        ThroughputError::Io { source, .. }
            if source.kind() == std::io::ErrorKind::PermissionDenied =>
        {
            ErrorCategory::Permission
        }
        _ => ErrorCategory::Connectivity,
    };
    Error::new(category, e.to_string())
}

pub async fn execute_request(
    value: &Value,
    out: &Path,
    manager: &RunManager,
    cancel: &CancellationToken,
) -> Result<(Value, u8)> {
    execute_request_with_exit_code(
        value,
        out,
        manager,
        cancel,
        &std::sync::atomic::AtomicU8::new(130),
    )
    .await
}

pub async fn execute_request_with_exit_code(
    value: &Value,
    out: &Path,
    manager: &RunManager,
    cancel: &CancellationToken,
    code: &std::sync::atomic::AtomicU8,
) -> Result<(Value, u8)> {
    let preview = plan_request(value)?;
    let handle = manager.begin(count(&preview))?;
    execute_prepared_with_exit_code(value, out, handle, cancel, code).await
}

pub fn reserve_request(value: &Value, manager: &RunManager) -> Result<crate::manager::RunHandle> {
    let preview = plan_request(value)?;
    manager.begin(count(&preview))
}

pub async fn execute_prepared(
    value: &Value,
    out: &Path,
    handle: crate::manager::RunHandle,
    cancel: &CancellationToken,
) -> Result<(Value, u8)> {
    execute_prepared_with_exit_code(
        value,
        out,
        handle,
        cancel,
        &std::sync::atomic::AtomicU8::new(130),
    )
    .await
}

async fn execute_prepared_with_exit_code(
    value: &Value,
    out: &Path,
    handle: crate::manager::RunHandle,
    cancel: &CancellationToken,
    cancel_code: &std::sync::atomic::AtomicU8,
) -> Result<(Value, u8)> {
    let preview = plan_request(value)?;
    let req = request(value)?;
    let total = count(&preview);
    let run_id = handle.run_id.clone();
    let started = now();
    let elapsed = std::time::Instant::now();
    let root = out.join(&run_id);
    let lock_path = out.join(".measurement.lock");
    let _lock = lantern_platform::SidecarLock::acquire(&lock_path, std::time::Duration::ZERO)?;
    std::fs::create_dir_all(&root)?;
    lantern_platform::check_path(&root)?;
    let engine_cancel = handle.cancel.clone();
    if cancel.is_cancelled() {
        engine_cancel.cancel();
    }
    let external = cancel.clone();
    let relay_token = engine_cancel.clone();
    let relay = tokio::spawn(async move {
        external.cancelled().await;
        relay_token.cancel();
    });
    let mut execution = Execution {
        native: crate::native::NativeExecution::new(&run_id).await?,
        handle: &handle,
        root: &root,
        cancel: &engine_cancel,
        completed: 0,
        failed: 0,
        skipped: 0,
        threshold_breaches: 0,
        warnings: Vec::new(),
    };
    let result = execution.run(&req, &preview).await;
    relay.abort();
    let cancelled = engine_cancel.is_cancelled();
    let code = if cancelled {
        cancel_code.load(std::sync::atomic::Ordering::SeqCst)
    } else if execution.completed == 0
        && let Err(error) = &result
    {
        error.category.throughput_exit_code()
    } else if result.is_err() || execution.failed > 0 {
        if execution.completed > execution.failed {
            14
        } else {
            15
        }
    } else if execution.threshold_breaches > 0 {
        14
    } else {
        0
    };
    let code = if req.capability != "throughput" && !matches!(code, 0 | 130 | 143) {
        1
    } else {
        code
    };
    if let Err(error) = result {
        handle.progress(execution.completed, Some(&error.message));
        execution.warnings.push(error.message);
    }
    let status = if cancelled {
        "Cancelled"
    } else if code == 0 {
        "Success"
    } else if execution.completed > execution.failed {
        "PartialFailure"
    } else {
        "TotalFailure"
    };
    let summary = json!({"schema_version":RECORD_VERSION,"provenance":Provenance::default(),"run_id":run_id,"timestamp":started,"completed_utc":now(),"elapsed_seconds":elapsed.elapsed().as_secs_f64(),"status":status,"exit_code":code,"counts":{"planned":total,"total":execution.completed,"failed":execution.failed,"succeeded":execution.completed.saturating_sub(execution.failed),"skipped":execution.skipped},"threshold_breach_count":execution.threshold_breaches,"warnings":execution.warnings,"plan":preview});
    let path = root.join("summary.json");
    if let Err(error) = atomic_json(&path, &summary, REPORT_LIMIT) {
        handle.finish(16, None);
        return Err(error);
    }
    let summary_path = path.to_string_lossy().to_string();
    handle.finish_outcome(code, Some(summary_path), status == "PartialFailure");
    Ok((summary, code))
}
fn count(preview: &Value) -> u64 {
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
struct Execution<'a> {
    native: crate::native::NativeExecution,
    handle: &'a crate::manager::RunHandle,
    root: &'a Path,
    cancel: &'a CancellationToken,
    completed: u64,
    failed: u64,
    skipped: u64,
    threshold_breaches: u64,
    warnings: Vec<String>,
}
impl Execution<'_> {
    async fn record(&mut self, value: Value, failed: bool) -> Result<()> {
        self.completed += 1;
        if failed {
            self.failed += 1;
        }
        atomic_json(
            &self
                .root
                .join(format!("measurement-{:08}.json", self.completed)),
            &json!({"schema_version":RECORD_VERSION,"provenance":Provenance::default(),"measurement":value}),
            REPORT_LIMIT,
        )?;
        self.handle.progress(
            self.completed,
            Some(if failed {
                "Measurement failed"
            } else {
                "Measurement complete"
            }),
        );
        Ok(())
    }
    fn run<'a>(
        &'a mut self,
        req: &'a Request,
        preview: &'a Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            if self.cancel.is_cancelled() {
                return Err(Error::new(ErrorCategory::Cancelled, "Run cancelled"));
            }
            match req.capability.as_str() {
                "throughput" => {
                    let (mut plan, _, thresholds) = throughput(&req.layers, req.strict)?;
                    self.handle.progress(
                        self.completed,
                        Some("Checking native reachability, TCP port and MTU"),
                    );
                    let (address, preflight) =
                        self.native.preflight(&plan.config, self.cancel).await?;
                    atomic_json(
                        &self
                            .root
                            .join(format!("preflight-{:08}.json", self.completed)),
                        &preflight,
                        REPORT_LIMIT,
                    )?;
                    plan.config.target = address.to_string();
                    plan.config.ip_version = if address.is_ipv4() {
                        lantern_throughput::IpVersion::Ipv4
                    } else {
                        lantern_throughput::IpVersion::Ipv6
                    };
                    let client = lantern_throughput::Iperf3Client::with_config(&plan.config)
                        .map_err(throughput_error)?;
                    let mut saturated = HashSet::new();
                    for spec in plan.tests() {
                        if self.cancel.is_cancelled() {
                            break;
                        }
                        if spec.phase == lantern_throughput::TestPhase::UdpSaturation
                            && saturated.contains(&(spec.dscp, spec.direction))
                        {
                            self.skipped += 1;
                            continue;
                        }
                        match self
                            .native
                            .throughput(&client, &spec, &plan.config, self.cancel)
                            .await
                        {
                            Ok(result) => {
                                if spec.phase == lantern_throughput::TestPhase::UdpSaturation
                                    && result.loss_pct.is_some_and(|loss| {
                                        loss >= plan.config.udp_loss_threshold_pct
                                    })
                                {
                                    saturated.insert((spec.dscp, spec.direction));
                                }
                                let breaches = thresholds.breaches(&result);
                                if !breaches.is_empty() {
                                    self.threshold_breaches += 1;
                                }
                                self.record(
                                    json!({"result": result, "threshold_breaches": breaches}),
                                    false,
                                )
                                .await?;
                            }
                            Err(error) => {
                                if error.category == ErrorCategory::Cancelled {
                                    return Err(error);
                                }
                                self.record(json!({"spec":spec,"error":error}), true)
                                    .await?;
                            }
                        }
                    }
                }
                "path_basic" | "path_trace" => {
                    let (config, _) = path_config::resolve(
                        &req.layers,
                        req.strict,
                        req.capability == "path_trace",
                    )?;
                    if req.capability == "path_basic" {
                        let (plan, settings) = config.basic()?;
                        for item in plan.items() {
                            if self.cancel.is_cancelled() {
                                break;
                            }
                            match self.native.basic(&item, &settings, self.cancel).await {
                                Ok(result) => {
                                    let failed = result.status
                                        != lantern_path_basic::BasicRunStatus::Completed
                                        || !result.failed_stages.is_empty();
                                    self.record(json!(result), failed).await?;
                                }
                                Err(error) => {
                                    self.record(
                                        json!({"item":item,"error":error.to_string()}),
                                        true,
                                    )
                                    .await?
                                }
                            }
                        }
                    } else {
                        let (plan, settings) = config.trace()?;
                        for item in plan.items() {
                            if self.cancel.is_cancelled() {
                                break;
                            }
                            match self.native.trace(&item, &settings, self.cancel).await {
                                Ok(result) => {
                                    let failed = result.status
                                        != lantern_path_trace::TraceRunStatus::Completed
                                        || result.hops.is_empty()
                                        || result.hops.iter().all(|h| h.statistics.received == 0);
                                    self.record(json!(result), failed).await?;
                                }
                                Err(error) => {
                                    self.record(
                                        json!({"item":item,"error":error.to_string()}),
                                        true,
                                    )
                                    .await?
                                }
                            }
                        }
                    }
                }
                "tuning" => {
                    let (plan, _, hash) = tuning(&req.layers, req.strict)?;
                    let result = if plan.requires_helper {
                        let id = uuid::Uuid::parse_str(&self.handle.run_id)
                            .map_err(|e| Error::new(ErrorCategory::Internal, e.to_string()))?;
                        lantern_helper::HelperClient::new()
                            .execute_tuning(id, &hash, plan, self.cancel)
                            .await
                            .map_err(helper_error)?
                    } else {
                        lantern_tuning::TuningExecutor::new()
                            .execute(&plan, self.cancel)
                            .await
                            .map_err(tuning_error)?
                    };
                    let failed = !result.success;
                    self.record(json!(result), failed).await?;
                }
                "workflow" => {
                    for step in preview["steps"].as_array().unwrap() {
                        let capability = step["capability"].as_str().unwrap();
                        let layer = if capability == "throughput" {
                            let mut layer = step["plan"]["config"].clone();
                            layer["bidirectional"] =
                                step["plan"]["capabilities"]["bidirectional"].clone();
                            if let Some(thresholds) = step["thresholds"].as_object() {
                                for (k, v) in thresholds {
                                    layer[k] = v.clone();
                                }
                            }
                            layer
                        } else {
                            step["settings"].clone()
                        };
                        let sub = Request {
                            capability: capability.into(),
                            layers: vec![layer],
                            strict: true,
                            workflow: None,
                        };
                        self.run(&sub, step).await?;
                    }
                }
                _ => {
                    return Err(Error::new(
                        ErrorCategory::Prerequisite,
                        "Capability has no native execution implementation",
                    ));
                }
            }
            Ok(())
        })
    }
}
pub fn list_runs(directory: &Path, offset: usize, limit: usize) -> Result<Value> {
    if limit == 0 || limit > 100 {
        return Err(Error::validation("Page size must be 1–100"));
    }
    if !directory.try_exists()? {
        return Ok(json!({"runs":[],"offset":offset,"limit":limit,"total":0,"has_more":false}));
    }
    lantern_platform::check_path(directory)?;
    let mut paths = Vec::new();
    let mut path_bytes = 0usize;
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let summary = entry.path().join("summary.json");
            if summary.is_file() {
                retain_run_path(&mut paths, &mut path_bytes, summary)?;
            }
        } else if entry
            .file_name()
            .to_string_lossy()
            .starts_with("iperf3_summary_")
            && entry
                .path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            retain_run_path(&mut paths, &mut path_bytes, entry.path())?;
        }
    }
    paths.sort();
    paths.reverse();
    let total = paths.len();
    let mut runs = Vec::new();
    for path in paths.into_iter().skip(offset).take(limit) {
        match crate::reports::read(&path) {
            Ok(mut summary) => {
                summary.data = Value::Null;
                let row = json!({"path":path,"summary":summary});
                if serde_json::to_vec(&row)
                    .map_err(|error| Error::validation(error.to_string()))?
                    .len()
                    > 4096
                {
                    runs.push(json!({"path":path,"error":"Summary metadata exceeds the listing limit; open the report directly"}));
                } else {
                    runs.push(row);
                }
            }
            Err(error) => runs.push(json!({"path":path,"error":error})),
        }
    }
    let legacy = directory.join("iperf3_run_index.json");
    let index = if legacy.is_file() {
        match read_json(&legacy, PROFILE_LIMIT) {
            Ok(value) => {
                if serde_json::to_vec(&value)
                    .map_err(|error| Error::validation(error.to_string()))?
                    .len()
                    > 512 * 1024
                {
                    Some(
                        json!({"path":legacy,"error":"Legacy index exceeds listing limit; use runs show to read it directly"}),
                    )
                } else {
                    Some(value)
                }
            }
            Err(error) => Some(json!({"path":legacy,"error":error})),
        }
    } else {
        None
    };
    let page = json!({"runs":runs,"legacy_index":index,"offset":offset,"limit":limit,"total":total,"has_more":offset.saturating_add(limit)<total});
    if serde_json::to_vec(&page)
        .map_err(|error| Error::validation(error.to_string()))?
        .len()
        > 1024 * 1024
    {
        return Err(Error::validation(
            "Run listing exceeds 1 MiB; request fewer runs per page",
        ));
    }
    Ok(page)
}
fn retain_run_path(paths: &mut Vec<PathBuf>, bytes: &mut usize, path: PathBuf) -> Result<()> {
    let next = bytes.saturating_add(path.as_os_str().len());
    if paths.len() >= 100_000 || next > REPORT_LIMIT {
        return Err(Error::validation(
            "Run directory exceeds listing limits of 100000 records or 16 MiB of paths; open a report directly or select a smaller directory",
        ));
    }
    *bytes = next;
    paths.push(path);
    Ok(())
}

pub fn profile_parameters(value: &Value) -> Result<Value> {
    let preview = plan_request(value)?;
    fn direct(preview: &Value) -> Value {
        if preview["capability"] == "throughput" {
            let mut parameters = preview["plan"]["config"].clone();
            parameters["bidirectional"] = preview["plan"]["capabilities"]["bidirectional"].clone();
            if let Some(thresholds) = preview["thresholds"].as_object() {
                for (k, v) in thresholds {
                    parameters[k] = v.clone();
                }
            }
            parameters
        } else {
            preview["settings"].clone()
        }
    }
    if preview["capability"] != "workflow" {
        return Ok(
            json!({"schema_version":1,"capability":preview["capability"],"parameters":direct(&preview)}),
        );
    }
    let mut parameters = json!({});
    for step in preview["steps"].as_array().unwrap() {
        let resolved = direct(step);
        if step["capability"] == "path_basic" {
            parameters["path"] = resolved;
        } else if step["capability"] == "throughput" {
            parameters["throughput"] = resolved;
            if parameters["throughput"]["max_total_tests"].is_null() {
                parameters["throughput"]["max_total_tests"] = json!(0);
            }
        } else if step["capability"] == "tuning" {
            parameters["windowsTuning"] = resolved;
        } else {
            return Err(Error::validation("Unknown workflow capability"));
        }
    }
    Ok(
        json!({"schema_version":1,"capability":"workflow","workflow":preview["workflow"],"parameters":parameters}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_run_listing_does_not_duplicate_csv_sidecars() {
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        std::fs::write(
            dir.path().join("iperf3_summary_fixture.json"),
            r#"{"SummaryVersion":2,"Status":"Success","Counts":{"Total":1,"Failed":0}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("iperf3_summary_fixture.csv"),
            "No,Status\n1,Success\n",
        )
        .unwrap();
        let listed = list_runs(dir.path(), 0, 20).unwrap();
        assert_eq!(listed["total"], 1);
        assert_eq!(listed["runs"][0]["summary"]["status"], "Success");
    }
    #[test]
    fn resolved_profiles_round_trip_without_losing_nested_overrides() {
        for request in [
            json!({"capability":"throughput","layers":[{"target":"fixture.invalid","duration_secs":2,"single_test":true,"bidirectional":false},{"omit_secs":0,"max_loss_pct":2.5}]}),
            json!({"capability":"path_trace","layers":[{"hosts_ipv4":["fixture.invalid"],"types":["TCP4"]}]}),
            json!({"capability":"workflow","workflow":"baseline","layers":[{"throughput":{"target":"fixture.invalid","port":5003,"duration_secs":3,"omit_secs":0},"path":{"skipPathping":true,"max_hops":4}},{"throughput":{"protocol":"TCP"}}]}),
        ] {
            let before = plan_request(&request).unwrap();
            let envelope = profile_parameters(&request).unwrap();
            crate::profiles::validate_parameters(&envelope).unwrap();
            let after=plan_request(&json!({"capability":request["capability"],"workflow":request.get("workflow"),"layers":[envelope],"strict":true})).unwrap();
            // Explicit resolved defaults can remove a warning, but execution plans must match.
            assert_eq!(before["plan"], after["plan"]);
            assert_eq!(before["settings"], after["settings"]);
            assert_eq!(before["steps"], after["steps"]);
        }
    }
    #[test]
    fn single_family_path_targets_survive_resolved_settings_and_profile_storage() {
        let directory =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let store = crate::profiles::ProfileStore {
            path: directory.path().join("profiles.json"),
        };
        for capability in ["path_basic", "path_trace"] {
            for (target, excluded) in [("192.0.2.1", "hosts_ipv6"), ("2001:db8::1", "hosts_ipv4")] {
                let request = json!({"capability":capability,"layers":[{"target":target}]});
                let preview = plan_request(&request).unwrap();
                assert_eq!(preview["settings"][excluded], json!([]));
                assert_eq!(
                    count(&preview),
                    if capability == "path_basic" { 1 } else { 2 }
                );

                // The desktop executes the resolved settings from its reviewed preview.
                let replanned = plan_request(&json!({
                    "capability":capability,"layers":[preview["settings"]],"strict":true
                }))
                .unwrap();
                assert_eq!(preview["plan"], replanned["plan"]);
                assert_eq!(preview["settings"], replanned["settings"]);

                let envelope = profile_parameters(&request).unwrap();
                store.save("single-target", &envelope).unwrap();
                let loaded = store.get("single-target").unwrap();
                let restored = plan_request(&json!({
                    "capability":capability,"layers":[loaded],"strict":true
                }))
                .unwrap();
                assert_eq!(preview["plan"], restored["plan"]);
                assert_eq!(preview["settings"], restored["settings"]);
            }
        }
    }
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
                    assert_eq!(error.category, ErrorCategory::Validation);
                    assert!(error.message.contains("throughput"));
                }
            }
        }
    }
    #[test]
    fn unreadable_legacy_index_preserves_valid_run_listing() {
        let directory =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        std::fs::write(
            directory.path().join("iperf3_summary_fixture.json"),
            r#"{"SummaryVersion":2,"Status":"Success","Counts":{"Total":1,"Failed":0}}"#,
        )
        .unwrap();
        let index = directory.path().join("iperf3_run_index.json");
        for contents in [b"broken".to_vec(), vec![b' '; PROFILE_LIMIT + 1]] {
            std::fs::write(&index, contents).unwrap();
            let page = list_runs(directory.path(), 0, 20).unwrap();
            assert_eq!(page["total"], 1);
            assert_eq!(page["runs"][0]["summary"]["status"], "Success");
            assert_eq!(page["legacy_index"]["path"], json!(index));
            assert!(page["legacy_index"]["error"]["message"].is_string());
        }
    }
    #[test]
    fn profile_capability_mismatch_is_rejected_before_planning() {
        assert!(plan_request(&json!({"capability":"path_basic","layers":[{"schema_version":1,"capability":"throughput","parameters":{}}]})).is_err());
    }
    #[tokio::test]
    async fn signal_reason_matches_partial_summary() {
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let request =
            json!({"capability":"throughput","layers":[{"target":"127.0.0.1","single_test":true}]});
        let cancel = CancellationToken::new();
        cancel.cancel();
        let code = std::sync::atomic::AtomicU8::new(143);
        let (summary, status) = execute_request_with_exit_code(
            &request,
            dir.path(),
            &RunManager::default(),
            &cancel,
            &code,
        )
        .await
        .unwrap();
        assert_eq!(status, 143);
        assert_eq!(summary["exit_code"], 143);
        assert_eq!(summary["status"], "Cancelled");
        assert_eq!(summary["counts"]["total"], 0);
    }
    #[test]
    fn directory_listing_bounds_retained_paths_before_push() {
        let mut paths = Vec::new();
        let mut bytes = REPORT_LIMIT;
        assert!(retain_run_path(&mut paths, &mut bytes, PathBuf::from("one")).is_err());
        assert!(paths.is_empty());
        assert_eq!(bytes, REPORT_LIMIT);
        bytes = 0;
        for _ in 0..100_000 {
            retain_run_path(&mut paths, &mut bytes, PathBuf::from("x")).unwrap();
        }
        assert!(retain_run_path(&mut paths, &mut bytes, PathBuf::from("x")).is_err());
        assert_eq!(paths.len(), 100_000);
    }
    #[test]
    fn directory_listing_does_not_accumulate_oversized_summary_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().canonicalize().unwrap();
        for index in 0..3 {
            std::fs::write(
                directory.join(format!("iperf3_summary_{index}.json")),
                serde_json::to_vec(
                    &json!({"provenance":{"detail":"x".repeat(100_000)},"status":"Success"}),
                )
                .unwrap(),
            )
            .unwrap();
        }
        let page = list_runs(&directory, 0, 20).unwrap();
        assert_eq!(page["total"], 3);
        assert_eq!(page["runs"].as_array().unwrap().len(), 3);
        assert!(
            page["runs"][0]["error"]
                .as_str()
                .unwrap()
                .contains("open the report")
        );
        assert!(serde_json::to_vec(&page).unwrap().len() < 4096);
    }
}
