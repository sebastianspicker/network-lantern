//! Runs one reserved request and publishes its measurement records and summary.
use crate::{
    application::{Capability, Request, count, plan_request, request, throughput, tuning},
    errors::{helper_error, internal, throughput_error, tuning_error},
    manager::RunManager,
    path_config,
};
use lantern_contracts::{Error, ErrorCategory, Provenance, RECORD_VERSION, Result, exit, now};
use lantern_platform::{REPORT_LIMIT, atomic_json};
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path};
use tokio_util::sync::CancellationToken;

/// `summary.json`; fields are declared in serialized key order.
#[derive(Serialize)]
struct Summary<'a> {
    completed_utc: String,
    counts: Counts,
    elapsed_seconds: f64,
    exit_code: u8,
    plan: &'a Value,
    provenance: Provenance,
    run_id: &'a str,
    schema_version: u32,
    status: &'static str,
    threshold_breach_count: u64,
    timestamp: &'a str,
    warnings: &'a [String],
}
#[derive(Serialize)]
struct Counts {
    failed: u64,
    planned: u64,
    skipped: u64,
    succeeded: u64,
    total: u64,
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
        &std::sync::atomic::AtomicU8::new(exit::CANCELLED),
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
        &std::sync::atomic::AtomicU8::new(exit::CANCELLED),
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
            exit::PARTIAL_FAILURE
        } else {
            exit::TOTAL_FAILURE
        }
    } else if execution.threshold_breaches > 0 {
        exit::PARTIAL_FAILURE
    } else {
        exit::SUCCESS
    };
    let code = exit::for_capability(req.capability == Capability::Throughput, code);
    if let Err(error) = result {
        handle.progress(execution.completed, Some(&error.message));
        execution.warnings.push(error.message);
    }
    let status = if cancelled {
        "Cancelled"
    } else if code == exit::SUCCESS {
        "Success"
    } else if execution.completed > execution.failed {
        "PartialFailure"
    } else {
        "TotalFailure"
    };
    let summary = serde_json::to_value(Summary {
        completed_utc: now(),
        counts: Counts {
            failed: execution.failed,
            planned: total,
            skipped: execution.skipped,
            succeeded: execution.completed.saturating_sub(execution.failed),
            total: execution.completed,
        },
        elapsed_seconds: elapsed.elapsed().as_secs_f64(),
        exit_code: code,
        plan: &preview,
        provenance: Provenance::default(),
        run_id: &run_id,
        schema_version: RECORD_VERSION,
        status,
        threshold_breach_count: execution.threshold_breaches,
        timestamp: &started,
        warnings: &execution.warnings,
    })
    .map_err(internal)?;
    let path = root.join("summary.json");
    if let Err(error) = atomic_json(&path, &summary, REPORT_LIMIT) {
        handle.finish(exit::INTERNAL, None);
        return Err(error);
    }
    let summary_path = path.to_string_lossy().to_string();
    handle.finish_outcome(code, Some(summary_path), status == "PartialFailure");
    Ok((summary, code))
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
            &json!({
                "schema_version":RECORD_VERSION,"provenance":Provenance::default(),"measurement":value
            }),
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
            match req.capability {
                Capability::Throughput => {
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
                Capability::PathBasic | Capability::PathTrace => {
                    let (config, _) = path_config::resolve(
                        &req.layers,
                        req.strict,
                        req.capability == Capability::PathTrace,
                    )?;
                    if req.capability == Capability::PathBasic {
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
                Capability::Tuning => {
                    let (plan, _, hash) = tuning(&req.layers, req.strict)?;
                    let result = if plan.requires_helper {
                        let id = uuid::Uuid::parse_str(&self.handle.run_id).map_err(internal)?;
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
                Capability::Workflow => {
                    for step in preview["steps"].as_array().unwrap() {
                        let capability = Capability::parse(step["capability"].as_str().unwrap())?;
                        let layer = if capability == Capability::Throughput {
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
                            capability,
                            layers: vec![layer],
                            strict: true,
                            workflow: None,
                        };
                        self.run(&sub, step).await?;
                    }
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn key_paths(value: &Value, prefix: &str, paths: &mut std::collections::BTreeSet<String>) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    let path = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    paths.insert(path.clone());
                    key_paths(child, &path, paths);
                }
            }
            Value::Array(items) => {
                for item in items {
                    key_paths(item, &format!("{prefix}[]"), paths);
                }
            }
            _ => {}
        }
    }
    #[tokio::test]
    async fn summary_key_set_matches_golden() {
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let request = json!({"capability":"throughput","layers":[{"target":"fixture.invalid","single_test":true}]});
        let cancel = CancellationToken::new();
        cancel.cancel();
        let (summary, _) = execute_request(&request, dir.path(), &RunManager::default(), &cancel)
            .await
            .unwrap();
        let written: Value = serde_json::from_slice(
            &std::fs::read(
                dir.path()
                    .join(summary["run_id"].as_str().unwrap())
                    .join("summary.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(written, summary);
        let mut paths = std::collections::BTreeSet::new();
        key_paths(&written, "", &mut paths);
        let golden = include_str!("../tests/fixtures/summary-keys.txt")
            .lines()
            .map(str::to_owned)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(paths, golden);
    }
}
