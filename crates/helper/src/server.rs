use crate::{
    HelperError, Result,
    protocol::{
        AuthorizedRequest, HelperOperation, OperationResult, ProbeOperation, ProbeResult,
        ReplayCache, SignedFrame, verify,
    },
};
use lantern_path_io::RoundParameters;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerIdentity {
    pub(crate) user_id: u64,
    pub(crate) process_id: Option<u32>,
    pub(crate) trusted_signature: bool,
}

/// Authenticates and dispatches one operation at a time for a platform transport.
#[derive(Debug, Clone)]
pub struct HelperServer {
    secret: [u8; 32],
    owner_user_id: u64,
    replay: Arc<Mutex<ReplayCache>>,
    admission: Arc<Mutex<AdmissionState>>,
}

const CANCELLATION_CLEANUP_TIMEOUT: Duration = Duration::from_secs(8);
const PENDING_CANCELLATION_TTL: Duration = Duration::from_secs(300);
const MAX_PENDING_CANCELLATIONS: usize = 1_024;

#[derive(Debug, Default)]
struct AdmissionState {
    active: HashMap<Uuid, CancellationToken>,
    pending_cancellations: HashMap<Uuid, Instant>,
    quarantined: bool,
}

struct ActiveRunGuard {
    run_id: Uuid,
    cancellation: CancellationToken,
    admission: Arc<Mutex<AdmissionState>>,
    finished: bool,
}

impl ActiveRunGuard {
    fn complete(mut self) -> Result<()> {
        let mut admission = self
            .admission
            .lock()
            .map_err(|_| HelperError::Protocol("active run lock poisoned".into()))?;
        admission.active.remove(&self.run_id);
        self.finished = true;
        Ok(())
    }
}

impl Drop for ActiveRunGuard {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Ok(mut admission) = self.admission.lock() {
            admission.active.remove(&self.run_id);
            if !self.finished {
                admission.quarantined = true;
            }
        }
    }
}

impl HelperServer {
    pub fn new(secret: [u8; 32], owner_user_id: u64) -> Self {
        Self {
            secret,
            owner_user_id,
            replay: Arc::new(Mutex::new(ReplayCache::default())),
            admission: Arc::new(Mutex::new(AdmissionState::default())),
        }
    }

    pub(crate) async fn handle(
        &self,
        frame: SignedFrame,
        peer: PeerIdentity,
        disconnected: CancellationToken,
    ) -> Result<OperationResult> {
        if peer.user_id != self.owner_user_id || !peer.trusted_signature {
            return Err(HelperError::Authorization(
                "transport peer is not the registered application owner".into(),
            ));
        }
        verify(&self.secret, &frame)?;
        self.replay
            .lock()
            .map_err(|_| HelperError::Protocol("replay cache lock poisoned".into()))?
            .accept(frame.nonce)?;
        let request: AuthorizedRequest = serde_json::from_slice(&frame.body)
            .map_err(|error| HelperError::Protocol(format!("decode request: {error}")))?;
        request.validate()?;
        let cancellation = disconnected.child_token();
        let active_guard = self.admit(request.run_id, cancellation.clone())?;
        let execution = dispatch(request.operation, &cancellation);
        tokio::pin!(execution);
        let result = tokio::select! {
            value = &mut execution => value,
            _ = disconnected.cancelled() => {
                cancellation.cancel();
                match tokio::time::timeout(CANCELLATION_CLEANUP_TIMEOUT, &mut execution).await {
                    Ok(value) => value,
                    Err(_) => return Err(HelperError::Scope(
                        "helper quarantined after cancellation cleanup timed out".into(),
                    )),
                }
            },
        };
        active_guard.complete()?;
        result
    }

    fn admit(&self, run_id: Uuid, cancellation: CancellationToken) -> Result<ActiveRunGuard> {
        let mut admission = self
            .admission
            .lock()
            .map_err(|_| HelperError::Protocol("active run lock poisoned".into()))?;
        let now = Instant::now();
        admission
            .pending_cancellations
            .retain(|_, expires| *expires > now);
        if admission.quarantined {
            return Err(HelperError::Scope(
                "helper is quarantined until the service restarts".into(),
            ));
        }
        if admission.pending_cancellations.remove(&run_id).is_some() {
            cancellation.cancel();
            return Err(HelperError::Cancelled);
        }
        if !admission.active.is_empty() {
            return Err(HelperError::Scope(
                "another helper operation is active".into(),
            ));
        }
        admission.active.insert(run_id, cancellation.clone());
        drop(admission);
        Ok(ActiveRunGuard {
            run_id,
            cancellation,
            admission: Arc::clone(&self.admission),
            finished: false,
        })
    }

    pub fn cancel_run(&self, run_id: Uuid) -> bool {
        let Ok(mut admission) = self.admission.lock() else {
            return false;
        };
        let now = Instant::now();
        admission
            .pending_cancellations
            .retain(|_, expires| *expires > now);
        if let Some(token) = admission.active.get(&run_id) {
            token.cancel();
            return true;
        }
        if admission.pending_cancellations.len() >= MAX_PENDING_CANCELLATIONS {
            admission.quarantined = true;
            return false;
        }
        admission
            .pending_cancellations
            .insert(run_id, now + PENDING_CANCELLATION_TTL);
        false
    }

    /// Builds the service instance from the platform-owned credential and owner identity.
    pub fn platform_default() -> Result<Self> {
        crate::platform::server_default()
    }

    /// Runs the protected platform transport until the service is stopped.
    pub fn run(self) -> Result<()> {
        crate::platform::run(self)
    }

    #[cfg(windows)]
    #[doc(hidden)]
    pub fn install_platform_service(owner_sid: &str) -> Result<()> {
        crate::platform::install_service(owner_sid)
    }

    #[cfg(windows)]
    #[doc(hidden)]
    pub fn remove_platform_service() -> Result<()> {
        crate::platform::remove_service()
    }
}

async fn dispatch(
    operation: HelperOperation,
    cancellation: &CancellationToken,
) -> Result<OperationResult> {
    match operation {
        HelperOperation::ExecuteTuning { plan } => {
            let value = lantern_tuning::TuningExecutor::new()
                .execute(&plan, cancellation)
                .await?;
            Ok(OperationResult::Tuning(value))
        }
        HelperOperation::RunDiagnostics { probes } => {
            let mut results = Vec::with_capacity(probes.len());
            for probe in probes {
                if cancellation.is_cancelled() {
                    return Err(HelperError::Cancelled);
                }
                results.push(run_probe(probe, cancellation).await?);
            }
            Ok(OperationResult::Diagnostics(results))
        }
        HelperOperation::BasicPath { item, settings } => {
            lantern_path_basic::execute_item(&item, &settings, cancellation)
                .await
                .map(OperationResult::BasicPath)
                .map_err(probe_error)
        }
        HelperOperation::TracePath { item, settings } => {
            lantern_path_trace::execute_run(&item, &settings, cancellation)
                .await
                .map(OperationResult::TracePath)
                .map_err(probe_error)
        }
        HelperOperation::ThroughputPreflight { config } => {
            let (target, evidence) =
                lantern_throughput::preflight_check(&config, cancellation).await?;
            Ok(OperationResult::ThroughputPreflight { target, evidence })
        }
        HelperOperation::ThroughputMeasurement { spec, limits } => limits
            .client()?
            .run_test(&spec, cancellation)
            .await
            .map(OperationResult::ThroughputMeasurement)
            .map_err(throughput_error),
    }
}

async fn run_probe(
    request: crate::ProbeRequest,
    cancellation: &CancellationToken,
) -> Result<ProbeResult> {
    let timeout = Duration::from_millis(u64::from(request.timeout_ms));
    let parameters = move |payload_size, max_hops, dont_fragment| RoundParameters {
        payload_size,
        dont_fragment,
        max_hops,
        timeout,
        traffic_class: request.traffic_class,
    };
    match request.operation {
        ProbeOperation::TcpConnect { port } => {
            lantern_path_io::test_tcp(request.target, port, timeout, cancellation)
                .await
                .map(ProbeResult::TcpConnected)
                .map_err(probe_error)
        }
        operation => {
            let target = request.target;
            let token = cancellation.clone();
            tokio::task::spawn_blocking(move || match operation {
                ProbeOperation::Ping {
                    count,
                    payload_bytes,
                    dont_fragment,
                } => lantern_path_io::ping(
                    target,
                    count,
                    64,
                    &parameters(payload_bytes.into(), 1, dont_fragment),
                    &token,
                )
                .map(ProbeResult::Samples),
                ProbeOperation::IcmpTrace {
                    first_ttl,
                    max_hops,
                    payload_bytes,
                } => lantern_path_io::trace(
                    target,
                    first_ttl,
                    &parameters(payload_bytes.into(), max_hops, false),
                    &token,
                )
                .map(ProbeResult::Hops),
                ProbeOperation::UdpTrace {
                    first_ttl,
                    max_hops,
                    payload_bytes,
                    base_port,
                } => lantern_path_io::udp_trace(
                    target,
                    first_ttl,
                    &parameters(payload_bytes.into(), max_hops, false),
                    base_port,
                    &token,
                )
                .map(ProbeResult::Hops),
                ProbeOperation::TcpTrace {
                    first_ttl,
                    max_hops,
                    port,
                } => lantern_path_io::tcp_trace(
                    target,
                    first_ttl,
                    &parameters(0, max_hops, false),
                    port,
                    &token,
                )
                .map(ProbeResult::Hops),
                ProbeOperation::TcpConnect { .. } => unreachable!(),
            })
            .await
            .map_err(|error| HelperError::Probe(format!("probe worker failed: {error}")))?
            .map_err(probe_error)
        }
    }
}

fn probe_error(error: lantern_path_io::ProbeError) -> HelperError {
    use lantern_contracts::ErrorCategory;
    use lantern_path_io::ProbeError;
    match error {
        ProbeError::Cancelled => HelperError::Cancelled,
        ProbeError::InvalidHost(message) | ProbeError::InvalidPlan(message) => {
            HelperError::Validation(message)
        }
        error @ (ProbeError::Dns { .. }
        | ProbeError::AddressFamilyUnavailable { .. }
        | ProbeError::TimedOut(_)) => HelperError::Diagnostic(lantern_contracts::Error::new(
            ErrorCategory::Connectivity,
            error.to_string(),
        )),
        error @ ProbeError::Permission { .. } => HelperError::Diagnostic(
            lantern_contracts::Error::new(ErrorCategory::Permission, error.to_string()),
        ),
        error @ (ProbeError::Unsupported { .. } | ProbeError::Io { .. }) => {
            HelperError::Diagnostic(lantern_contracts::Error::new(
                ErrorCategory::Prerequisite,
                error.to_string(),
            ))
        }
    }
}

fn throughput_error(error: lantern_throughput::ThroughputError) -> HelperError {
    use lantern_throughput::ThroughputError as T;
    match error {
        T::Cancelled => HelperError::Cancelled,
        T::Validation(message) => HelperError::Validation(message),
        T::BudgetExceeded { .. } => HelperError::Validation(error.to_string()),
        T::Io { source, .. } if source.kind() == std::io::ErrorKind::PermissionDenied => {
            HelperError::Authorization(format!(
                "Windows qWAVE DSCP setup requires an administrator or Network Configuration Operators token: {source}"
            ))
        }
        other => HelperError::Probe(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{reviewed_hash, sign};
    use lantern_tuning::{TuningConfig, plan};
    use rand::RngCore;

    fn frame(secret: &[u8; 32], run_id: Uuid) -> SignedFrame {
        let plan = plan(&TuningConfig {
            dry_run: true,
            ..Default::default()
        })
        .unwrap();
        let operation = HelperOperation::ExecuteTuning { plan };
        let request = AuthorizedRequest {
            run_id,
            reviewed_plan_hash: reviewed_hash(&operation).unwrap(),
            operation,
        };
        let mut nonce = [0; 32];
        rand::rng().fill_bytes(&mut nonce);
        sign(secret, nonce, serde_json::to_vec(&request).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn peer_scope_and_disconnect_are_enforced() {
        let secret = [2; 32];
        let server = HelperServer::new(secret, 501);
        let denied = server
            .handle(
                frame(&secret, Uuid::new_v4()),
                PeerIdentity {
                    user_id: 502,
                    process_id: None,
                    trusted_signature: true,
                },
                CancellationToken::new(),
            )
            .await;
        assert!(matches!(denied, Err(HelperError::Authorization(_))));
        let disconnected = CancellationToken::new();
        disconnected.cancel();
        let cancelled = server
            .handle(
                frame(&secret, Uuid::new_v4()),
                PeerIdentity {
                    user_id: 501,
                    process_id: Some(7),
                    trusted_signature: true,
                },
                disconnected,
            )
            .await;
        assert!(matches!(cancelled, Err(HelperError::Cancelled)));
        let guard = server
            .admit(Uuid::new_v4(), CancellationToken::new())
            .expect("graceful cancellation must release admission");
        guard.complete().unwrap();
    }

    #[test]
    fn graceful_completion_releases_admission() {
        let server = HelperServer::new([9; 32], 501);
        let first_id = Uuid::new_v4();
        let first_token = CancellationToken::new();
        let guard = server.admit(first_id, first_token.clone()).unwrap();
        assert!(matches!(
            server.admit(Uuid::new_v4(), CancellationToken::new()),
            Err(HelperError::Scope(_))
        ));
        guard.complete().unwrap();
        assert!(first_token.is_cancelled());
        let next = server
            .admit(Uuid::new_v4(), CancellationToken::new())
            .unwrap();
        next.complete().unwrap();
    }

    #[test]
    fn dropping_active_run_guard_quarantines_admission() {
        let server = HelperServer::new([9; 32], 501);
        let token = CancellationToken::new();
        drop(server.admit(Uuid::new_v4(), token.clone()).unwrap());
        assert!(token.is_cancelled());
        assert!(matches!(
            server.admit(Uuid::new_v4(), CancellationToken::new()),
            Err(HelperError::Scope(message)) if message.contains("quarantined")
        ));
    }

    #[test]
    fn cancellation_before_admission_prevents_late_start() {
        let server = HelperServer::new([9; 32], 501);
        let cancelled_id = Uuid::new_v4();
        assert!(!server.cancel_run(cancelled_id));
        assert!(matches!(
            server.admit(cancelled_id, CancellationToken::new()),
            Err(HelperError::Cancelled)
        ));
        let next = server
            .admit(Uuid::new_v4(), CancellationToken::new())
            .unwrap();
        next.complete().unwrap();
    }

    #[test]
    fn pending_cancellation_overflow_quarantines_admission() {
        let server = HelperServer::new([9; 32], 501);
        for _ in 0..=MAX_PENDING_CANCELLATIONS {
            assert!(!server.cancel_run(Uuid::new_v4()));
        }
        assert!(matches!(
            server.admit(Uuid::new_v4(), CancellationToken::new()),
            Err(HelperError::Scope(message)) if message.contains("quarantined")
        ));
    }
}
