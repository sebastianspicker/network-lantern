//! Selects the authorized native helper once per run. Planning never enters this module.
use crate::application::{helper_error, throughput_error};
use lantern_contracts::{Error, ErrorCategory, Result};
use lantern_helper::{HelperClient, HelperOperation};
use tokio_util::sync::CancellationToken;

pub(crate) struct NativeExecution {
    run_id: uuid::Uuid,
    use_helper: bool,
}

impl NativeExecution {
    pub async fn new(run_id: &str) -> Result<Self> {
        let run_id = uuid::Uuid::parse_str(run_id)
            .map_err(|e| Error::new(ErrorCategory::Internal, e.to_string()))?;
        let use_helper = HelperClient::new()
            .status()
            .await
            .is_ok_and(|status| status.service_reachable && status.authorized);
        Ok(Self { run_id, use_helper })
    }

    pub async fn preflight(
        &self,
        config: &lantern_throughput::SuiteConfig,
        cancel: &CancellationToken,
    ) -> Result<(std::net::IpAddr, serde_json::Value)> {
        if self.use_helper {
            let operation = HelperOperation::ThroughputPreflight {
                config: config.clone(),
            };
            let hash = HelperClient::reviewed_hash(&operation).map_err(helper_error)?;
            HelperClient::new()
                .preflight(self.run_id, &hash, config.clone(), cancel)
                .await
                .map_err(helper_error)
        } else {
            lantern_throughput::preflight_check(config, cancel).await
        }
    }

    pub async fn throughput(
        &self,
        client: &lantern_throughput::Iperf3Client,
        spec: &lantern_throughput::TestSpec,
        config: &lantern_throughput::SuiteConfig,
        cancel: &CancellationToken,
    ) -> Result<lantern_throughput::TestResult> {
        let retry = config.retry;
        #[cfg(windows)]
        if spec.tos != 0 && self.use_helper {
            let limits = lantern_helper::ThroughputLimits::from(config);
            let operation = HelperOperation::ThroughputMeasurement {
                spec: spec.clone(),
                limits: limits.clone(),
            };
            let hash = HelperClient::reviewed_hash(&operation).map_err(helper_error)?;
            let attempts = retry.max_retries.saturating_add(1);
            for attempt in 1..=attempts {
                match HelperClient::new()
                    .execute_throughput_measurement(
                        self.run_id,
                        &hash,
                        spec.clone(),
                        limits.clone(),
                        cancel,
                    )
                    .await
                {
                    Ok(mut result) => {
                        result.attempts = attempt;
                        return Ok(result);
                    }
                    Err(error) => {
                        let retryable = matches!(
                            &error,
                            lantern_helper::HelperError::Probe(_)
                                | lantern_helper::HelperError::Transport { .. }
                                | lantern_helper::HelperError::Unavailable(_)
                        );
                        if retryable && attempt < attempts {
                            tokio::select! {
                                _ = cancel.cancelled() => {
                                    return Err(Error::new(ErrorCategory::Cancelled, "throughput run was cancelled"));
                                }
                                _ = tokio::time::sleep(std::time::Duration::from_millis(retry.backoff_ms)) => {}
                            }
                            continue;
                        }
                        let mapped = helper_error(error);
                        return if retryable && attempt > 1 {
                            Err(Error::new(
                                mapped.category,
                                format!("all {attempt} attempt(s) failed: {}", mapped.message),
                            ))
                        } else {
                            Err(mapped)
                        };
                    }
                }
            }
            unreachable!("the bounded helper attempt range always contains one attempt");
        }
        client
            .run_test_with_retry(spec, retry, cancel)
            .await
            .map_err(throughput_error)
    }

    pub async fn basic(
        &self,
        item: &lantern_path_basic::BasicPlanItem,
        settings: &lantern_path_basic::BasicSettings,
        cancel: &CancellationToken,
    ) -> Result<lantern_path_basic::BasicRunResult> {
        if self.use_helper {
            let operation = HelperOperation::BasicPath {
                item: item.clone(),
                settings: settings.clone(),
            };
            let hash = HelperClient::reviewed_hash(&operation).map_err(helper_error)?;
            HelperClient::new()
                .execute_basic(self.run_id, &hash, item.clone(), settings.clone(), cancel)
                .await
                .map_err(helper_error)
        } else {
            lantern_path_basic::execute_item(item, settings, cancel)
                .await
                .map_err(probe_error)
        }
    }

    pub async fn trace(
        &self,
        item: &lantern_path_trace::TracePlanItem,
        settings: &lantern_path_trace::TraceSettings,
        cancel: &CancellationToken,
    ) -> Result<lantern_path_trace::TraceRunResult> {
        if self.use_helper {
            let operation = HelperOperation::TracePath {
                item: item.clone(),
                settings: settings.clone(),
            };
            let hash = HelperClient::reviewed_hash(&operation).map_err(helper_error)?;
            HelperClient::new()
                .execute_trace(self.run_id, &hash, item.clone(), settings.clone(), cancel)
                .await
                .map_err(helper_error)
        } else {
            lantern_path_trace::execute_run(item, settings, cancel)
                .await
                .map_err(probe_error)
        }
    }
}

fn probe_error(error: lantern_path_io::ProbeError) -> Error {
    use lantern_path_io::ProbeError as P;
    let category = match &error {
        P::Cancelled => ErrorCategory::Cancelled,
        P::Permission { .. } => ErrorCategory::Permission,
        P::Unsupported { .. } => ErrorCategory::Prerequisite,
        P::InvalidHost(_) | P::InvalidPlan(_) => ErrorCategory::Validation,
        _ => ErrorCategory::Connectivity,
    };
    Error::new(category, error.to_string())
}
