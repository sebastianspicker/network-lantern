use crate::{
    HelperError, HelperStatus, ProbeRequest, Result, platform,
    protocol::{
        AuthorizedRequest, HelperOperation, OperationResult, ProbeResult, reviewed_hash, sign,
    },
};
use lantern_tuning::{TuningPlan, TuningResult};
use rand::RngCore;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Client for the installed, platform-native privileged helper.
#[derive(Debug, Default, Clone, Copy)]
pub struct HelperClient;

impl HelperClient {
    pub const fn new() -> Self {
        Self
    }

    pub async fn status(&self) -> Result<HelperStatus> {
        platform::status().await
    }
    pub async fn register(&self) -> Result<HelperStatus> {
        platform::register().await
    }
    pub async fn remove(&self) -> Result<HelperStatus> {
        platform::remove().await
    }

    pub async fn execute_tuning(
        &self,
        run_id: Uuid,
        reviewed_plan_hash: &str,
        plan: TuningPlan,
        cancellation: &CancellationToken,
    ) -> Result<TuningResult> {
        match self
            .execute(
                run_id,
                reviewed_plan_hash,
                HelperOperation::ExecuteTuning { plan },
                cancellation,
            )
            .await?
        {
            OperationResult::Tuning(result) => Ok(result),
            _ => Err(HelperError::Protocol(
                "helper returned the wrong result type".into(),
            )),
        }
    }

    pub async fn run_diagnostics(
        &self,
        run_id: Uuid,
        reviewed_plan_hash: &str,
        probes: Vec<ProbeRequest>,
        cancellation: &CancellationToken,
    ) -> Result<Vec<ProbeResult>> {
        match self
            .execute(
                run_id,
                reviewed_plan_hash,
                HelperOperation::RunDiagnostics { probes },
                cancellation,
            )
            .await?
        {
            OperationResult::Diagnostics(result) => Ok(result),
            _ => Err(HelperError::Protocol(
                "helper returned the wrong result type".into(),
            )),
        }
    }

    pub async fn execute_basic(
        &self,
        run_id: Uuid,
        reviewed_plan_hash: &str,
        item: lantern_path_basic::BasicPlanItem,
        settings: lantern_path_basic::BasicSettings,
        cancellation: &CancellationToken,
    ) -> Result<lantern_path_basic::BasicRunResult> {
        match self
            .execute(
                run_id,
                reviewed_plan_hash,
                HelperOperation::BasicPath { item, settings },
                cancellation,
            )
            .await?
        {
            OperationResult::BasicPath(result) => Ok(result),
            _ => Err(HelperError::Protocol(
                "helper returned the wrong result type".into(),
            )),
        }
    }

    pub async fn execute_trace(
        &self,
        run_id: Uuid,
        reviewed_plan_hash: &str,
        item: lantern_path_trace::TracePlanItem,
        settings: lantern_path_trace::TraceSettings,
        cancellation: &CancellationToken,
    ) -> Result<lantern_path_trace::TraceRunResult> {
        match self
            .execute(
                run_id,
                reviewed_plan_hash,
                HelperOperation::TracePath { item, settings },
                cancellation,
            )
            .await?
        {
            OperationResult::TracePath(result) => Ok(result),
            _ => Err(HelperError::Protocol(
                "helper returned the wrong result type".into(),
            )),
        }
    }

    pub async fn preflight(
        &self,
        run_id: Uuid,
        reviewed_plan_hash: &str,
        config: lantern_throughput::SuiteConfig,
        cancellation: &CancellationToken,
    ) -> Result<(std::net::IpAddr, serde_json::Value)> {
        match self
            .execute(
                run_id,
                reviewed_plan_hash,
                HelperOperation::ThroughputPreflight { config },
                cancellation,
            )
            .await?
        {
            OperationResult::ThroughputPreflight { target, evidence } => Ok((target, evidence)),
            _ => Err(HelperError::Protocol(
                "helper returned the wrong result type".into(),
            )),
        }
    }

    pub async fn execute_throughput_measurement(
        &self,
        run_id: Uuid,
        reviewed_plan_hash: &str,
        spec: lantern_throughput::TestSpec,
        limits: crate::ThroughputLimits,
        cancellation: &CancellationToken,
    ) -> Result<lantern_throughput::TestResult> {
        match self
            .execute(
                run_id,
                reviewed_plan_hash,
                HelperOperation::ThroughputMeasurement { spec, limits },
                cancellation,
            )
            .await?
        {
            OperationResult::ThroughputMeasurement(result) => Ok(result),
            _ => Err(HelperError::Protocol(
                "helper returned the wrong result type".into(),
            )),
        }
    }

    /// Computes the hash that the review UI must display and bind to a request.
    pub fn reviewed_hash(operation: &HelperOperation) -> Result<String> {
        reviewed_hash(operation)
    }

    async fn execute(
        &self,
        run_id: Uuid,
        reviewed_plan_hash: &str,
        operation: HelperOperation,
        cancellation: &CancellationToken,
    ) -> Result<OperationResult> {
        if cancellation.is_cancelled() {
            return Err(HelperError::Cancelled);
        }
        let request = AuthorizedRequest {
            run_id,
            reviewed_plan_hash: reviewed_plan_hash.into(),
            operation,
        };
        request.validate()?;
        let body = serde_json::to_vec(&request)
            .map_err(|error| HelperError::Protocol(format!("encode request: {error}")))?;
        let mut nonce = [0; 32];
        rand::rng().fill_bytes(&mut nonce);
        let frame = sign(&platform::load_secret()?, nonce, body)?;
        platform::request(frame, cancellation).await
    }
}
