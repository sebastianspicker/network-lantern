use crate::{HelperError, Result};
use hmac::{Hmac, Mac};
use lantern_path_io::{HopResult, Sample};
use lantern_tuning::{TuningPlan, TuningResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_PROBES_PER_REQUEST: usize = 256;
const MAX_NONCES: usize = 4096;
const FRAME_VALIDITY: Duration = Duration::from_secs(5 * 60);
const CLOCK_SKEW: Duration = Duration::from_secs(30);
const NONCE_TTL: Duration = Duration::from_secs(6 * 60);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorizedRequest {
    pub run_id: Uuid,
    /// SHA-256 of the complete reviewed plan, encoded as uppercase hexadecimal.
    pub reviewed_plan_hash: String,
    pub operation: HelperOperation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HelperOperation {
    ExecuteTuning {
        plan: TuningPlan,
    },
    RunDiagnostics {
        probes: Vec<ProbeRequest>,
    },
    BasicPath {
        item: lantern_path_basic::BasicPlanItem,
        settings: lantern_path_basic::BasicSettings,
    },
    TracePath {
        item: lantern_path_trace::TracePlanItem,
        settings: lantern_path_trace::TraceSettings,
    },
    ThroughputPreflight {
        config: lantern_throughput::SuiteConfig,
    },
    ThroughputMeasurement {
        spec: lantern_throughput::TestSpec,
        limits: ThroughputLimits,
    },
}

/// Reviewed client deadlines and allocation bounds for a privileged measurement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThroughputLimits {
    pub connect_timeout_ms: u64,
    pub control_timeout_ms: u64,
    pub max_control_json_bytes: usize,
}
impl From<&lantern_throughput::SuiteConfig> for ThroughputLimits {
    fn from(config: &lantern_throughput::SuiteConfig) -> Self {
        Self {
            connect_timeout_ms: config.connect_timeout_ms,
            control_timeout_ms: config.control_timeout_ms,
            max_control_json_bytes: config.max_control_json_bytes,
        }
    }
}
impl ThroughputLimits {
    pub(crate) fn client(&self) -> Result<lantern_throughput::Iperf3Client> {
        lantern_throughput::Iperf3Client::with_limits(
            self.connect_timeout_ms,
            self.control_timeout_ms,
            self.max_control_json_bytes,
        )
        .map_err(|error| HelperError::Validation(error.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProbeRequest {
    pub target: IpAddr,
    pub operation: ProbeOperation,
    pub timeout_ms: u32,
    pub traffic_class: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProbeOperation {
    Ping {
        count: u16,
        payload_bytes: u16,
        dont_fragment: bool,
    },
    IcmpTrace {
        first_ttl: u8,
        max_hops: u8,
        payload_bytes: u16,
    },
    UdpTrace {
        first_ttl: u8,
        max_hops: u8,
        payload_bytes: u16,
        base_port: u16,
    },
    TcpTrace {
        first_ttl: u8,
        max_hops: u8,
        port: u16,
    },
    TcpConnect {
        port: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ProbeResult {
    Samples(Vec<Sample>),
    Hops(Vec<HopResult>),
    TcpConnected(bool),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum OperationResult {
    Tuning(TuningResult),
    Diagnostics(Vec<ProbeResult>),
    BasicPath(lantern_path_basic::BasicRunResult),
    TracePath(lantern_path_trace::TraceRunResult),
    ThroughputPreflight {
        target: IpAddr,
        evidence: serde_json::Value,
    },
    ThroughputMeasurement(lantern_throughput::TestResult),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "category",
    content = "message",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum WireError {
    Validation(String),
    Authorization(String),
    Unavailable(String),
    Authentication,
    Replay,
    Scope(String),
    Protocol(String),
    Transport(String),
    Cancelled,
    Tuning(String),
    Probe(String),
    Diagnostic(lantern_contracts::Error),
}

impl From<HelperError> for WireError {
    fn from(error: HelperError) -> Self {
        match error {
            HelperError::Validation(message) => Self::Validation(message),
            HelperError::Authorization(message) => Self::Authorization(message),
            HelperError::Unavailable(message) => Self::Unavailable(message),
            HelperError::Authentication => Self::Authentication,
            HelperError::Replay => Self::Replay,
            HelperError::Scope(message) => Self::Scope(message),
            HelperError::Protocol(message) => Self::Protocol(message),
            HelperError::Transport { operation, source } => {
                Self::Transport(format!("{operation}: {source}"))
            }
            HelperError::Cancelled => Self::Cancelled,
            HelperError::Tuning(message) => Self::Tuning(message),
            HelperError::Probe(message) => Self::Probe(message),
            HelperError::Diagnostic(error) => Self::Diagnostic(error),
        }
    }
}

impl From<WireError> for HelperError {
    fn from(error: WireError) -> Self {
        match error {
            WireError::Validation(message) => Self::Validation(message),
            WireError::Authorization(message) => Self::Authorization(message),
            WireError::Unavailable(message) => Self::Unavailable(message),
            WireError::Authentication => Self::Authentication,
            WireError::Replay => Self::Replay,
            WireError::Scope(message) => Self::Scope(message),
            WireError::Protocol(message) => Self::Protocol(message),
            WireError::Transport(message) => {
                Self::transport("remote helper", std::io::Error::other(message))
            }
            WireError::Cancelled => Self::Cancelled,
            WireError::Tuning(message) => Self::Tuning(message),
            WireError::Probe(message) => Self::Probe(message),
            WireError::Diagnostic(error) => Self::Diagnostic(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SignedFrame {
    pub nonce: [u8; 32],
    pub issued_at_unix_ms: i64,
    pub body: Vec<u8>,
    pub authenticator: [u8; 32],
}

impl AuthorizedRequest {
    pub fn validate(&self) -> Result<()> {
        if !valid_hash(&self.reviewed_plan_hash) {
            return Err(HelperError::Validation(
                "reviewedPlanHash must be 64 uppercase hexadecimal characters".into(),
            ));
        }
        match &self.operation {
            HelperOperation::ExecuteTuning { plan } => {
                let canonical = lantern_tuning::plan(&plan.config)?;
                if canonical != *plan {
                    return Err(HelperError::Scope("tuning plan is not canonical".into()));
                }
            }
            HelperOperation::RunDiagnostics { probes } => validate_probes(probes)?,
            HelperOperation::BasicPath { item, settings } => {
                lantern_path_io::validate_host(&item.host).map_err(probe_validation)?;
                settings.validate().map_err(probe_validation)?;
            }
            HelperOperation::TracePath { item, settings } => {
                lantern_path_io::validate_host(&item.host).map_err(probe_validation)?;
                item.round
                    .apply(settings)
                    .validate_for_type(item.probe_type)
                    .map_err(probe_validation)?;
            }
            HelperOperation::ThroughputPreflight { config } => {
                lantern_throughput::plan(config, lantern_throughput::ServerCapabilities::default())
                    .map_err(|error| HelperError::Validation(error.to_string()))?;
            }
            HelperOperation::ThroughputMeasurement { spec, limits } => {
                limits.client()?;
                spec.validate()
                    .map_err(|error| HelperError::Validation(error.to_string()))?;
            }
        }
        let actual = reviewed_hash(&self.operation)?;
        if actual != self.reviewed_plan_hash {
            return Err(HelperError::Scope(
                "operation does not match reviewedPlanHash".into(),
            ));
        }
        Ok(())
    }
}

fn probe_validation(error: lantern_path_io::ProbeError) -> HelperError {
    HelperError::Validation(error.to_string())
}

pub fn reviewed_hash(operation: &HelperOperation) -> Result<String> {
    let bytes = serde_json::to_vec(operation)
        .map_err(|error| HelperError::Protocol(format!("serialize reviewed operation: {error}")))?;
    Ok(hex_upper(&Sha256::digest(bytes)))
}

fn validate_probes(probes: &[ProbeRequest]) -> Result<()> {
    if probes.is_empty() || probes.len() > MAX_PROBES_PER_REQUEST {
        return Err(HelperError::Validation(format!(
            "diagnostic request must contain 1..={MAX_PROBES_PER_REQUEST} probes"
        )));
    }
    for probe in probes {
        if !(1..=30_000).contains(&probe.timeout_ms) {
            return Err(HelperError::Validation(
                "probe timeout must be 1..=30000 ms".into(),
            ));
        }
        let (first, max, payload) = match probe.operation {
            ProbeOperation::Ping {
                count,
                payload_bytes,
                ..
            } => {
                if !(1..=100).contains(&count) {
                    return Err(HelperError::Validation("ping count must be 1..=100".into()));
                }
                (1, 1, payload_bytes)
            }
            ProbeOperation::IcmpTrace {
                first_ttl,
                max_hops,
                payload_bytes,
            }
            | ProbeOperation::UdpTrace {
                first_ttl,
                max_hops,
                payload_bytes,
                ..
            } => (first_ttl, max_hops, payload_bytes),
            ProbeOperation::TcpTrace {
                first_ttl,
                max_hops,
                port,
            } => {
                if port == 0 {
                    return Err(HelperError::Validation("TCP port must be nonzero".into()));
                }
                (first_ttl, max_hops, 0)
            }
            ProbeOperation::TcpConnect { port } => {
                if port == 0 {
                    return Err(HelperError::Validation("TCP port must be nonzero".into()));
                }
                (1, 1, 0)
            }
        };
        if first == 0 || max == 0 || first > max || max > 64 {
            return Err(HelperError::Validation(
                "probe TTL range must be within 1..=64".into(),
            ));
        }
        if payload > 65_000 {
            return Err(HelperError::Validation(
                "probe payload exceeds 65000 bytes".into(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn sign(secret: &[u8; 32], nonce: [u8; 32], body: Vec<u8>) -> Result<SignedFrame> {
    if body.len() > MAX_FRAME_BYTES {
        return Err(HelperError::Protocol("request exceeds 1 MiB".into()));
    }
    let issued_at_unix_ms = unix_millis()?;
    let authenticator = authenticator(secret, &nonce, issued_at_unix_ms, &body);
    Ok(SignedFrame {
        nonce,
        issued_at_unix_ms,
        body,
        authenticator,
    })
}

pub(crate) fn verify(secret: &[u8; 32], frame: &SignedFrame) -> Result<()> {
    if frame.body.len() > MAX_FRAME_BYTES {
        return Err(HelperError::Protocol("request exceeds 1 MiB".into()));
    }
    let now = unix_millis()?;
    let earliest = now.saturating_sub(i64::try_from(FRAME_VALIDITY.as_millis()).expect("bounded"));
    let latest = now.saturating_add(i64::try_from(CLOCK_SKEW.as_millis()).expect("bounded"));
    if frame.issued_at_unix_ms < earliest || frame.issued_at_unix_ms > latest {
        return Err(HelperError::Authentication);
    }
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret).map_err(|_| HelperError::Authentication)?;
    mac.update(&frame.nonce);
    mac.update(&frame.issued_at_unix_ms.to_be_bytes());
    mac.update(&frame.body);
    mac.verify_slice(&frame.authenticator)
        .map_err(|_| HelperError::Authentication)?;
    Ok(())
}

fn authenticator(
    secret: &[u8; 32],
    nonce: &[u8; 32],
    issued_at_unix_ms: i64,
    body: &[u8],
) -> [u8; 32] {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret).expect("SHA-256 accepts a 32-byte HMAC key");
    mac.update(nonce);
    mac.update(&issued_at_unix_ms.to_be_bytes());
    mac.update(body);
    mac.finalize().into_bytes().into()
}

fn unix_millis() -> Result<i64> {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HelperError::Authentication)?
        .as_millis();
    i64::try_from(value).map_err(|_| HelperError::Authentication)
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
}

fn hex_upper(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

#[derive(Debug, Default)]
pub(crate) struct ReplayCache {
    entries: HashMap<[u8; 32], Instant>,
    order: VecDeque<[u8; 32]>,
}

impl ReplayCache {
    pub(crate) fn accept(&mut self, nonce: [u8; 32]) -> Result<()> {
        let cutoff = Instant::now() - NONCE_TTL;
        while self
            .order
            .front()
            .is_some_and(|key| self.entries.get(key).is_some_and(|seen| *seen < cutoff))
        {
            if let Some(key) = self.order.pop_front() {
                self.entries.remove(&key);
            }
        }
        if self.entries.contains_key(&nonce) {
            return Err(HelperError::Replay);
        }
        if self.entries.len() >= MAX_NONCES {
            return Err(HelperError::Protocol(
                "replay cache capacity exhausted".into(),
            ));
        }
        self.entries.insert(nonce, Instant::now());
        self.order.push_back(nonce);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_replay_and_tampering() {
        let secret = [7; 32];
        let nonce = [9; 32];
        let frame = sign(&secret, nonce, b"body".to_vec()).unwrap();
        verify(&secret, &frame).unwrap();
        let mut cache = ReplayCache::default();
        cache.accept(nonce).unwrap();
        assert!(matches!(cache.accept(nonce), Err(HelperError::Replay)));
        let mut changed = frame;
        changed.body[0] ^= 1;
        assert!(matches!(
            verify(&secret, &changed),
            Err(HelperError::Authentication)
        ));
    }

    #[test]
    fn rejects_stale_and_future_frames() {
        let secret = [4; 32];
        let mut stale = sign(&secret, [5; 32], b"body".to_vec()).unwrap();
        stale.issued_at_unix_ms -= 10 * 60 * 1_000;
        stale.authenticator =
            authenticator(&secret, &stale.nonce, stale.issued_at_unix_ms, &stale.body);
        assert!(matches!(
            verify(&secret, &stale),
            Err(HelperError::Authentication)
        ));
        let mut future = sign(&secret, [6; 32], b"body".to_vec()).unwrap();
        future.issued_at_unix_ms += 60 * 1_000;
        future.authenticator = authenticator(
            &secret,
            &future.nonce,
            future.issued_at_unix_ms,
            &future.body,
        );
        assert!(matches!(
            verify(&secret, &future),
            Err(HelperError::Authentication)
        ));
    }

    #[test]
    fn wire_error_preserves_diagnostic_category() {
        let error = WireError::from(HelperError::Diagnostic(lantern_contracts::Error::new(
            lantern_contracts::ErrorCategory::Permission,
            "raw socket denied",
        )));
        let bytes = serde_json::to_vec(&error).unwrap();
        let decoded: WireError = serde_json::from_slice(&bytes).unwrap();
        match HelperError::from(decoded) {
            HelperError::Diagnostic(error) => {
                assert_eq!(error.category, lantern_contracts::ErrorCategory::Permission);
                assert_eq!(error.message, "raw socket denied");
            }
            other => panic!("unexpected helper error: {other}"),
        }
    }

    #[test]
    fn rejects_throughput_measurement_that_breaks_reviewed_dscp_binding() {
        let spec = lantern_throughput::TestSpec {
            id: 1,
            target: "127.0.0.1".into(),
            port: 5201,
            ip_version: lantern_throughput::IpVersion::Ipv4,
            protocol: lantern_throughput::Protocol::Tcp,
            direction: lantern_throughput::Direction::Tx,
            dscp: lantern_throughput::DscpClass::EF,
            tos: lantern_throughput::DscpClass::AF11.tos(),
            streams: 1,
            tcp_window_bytes: None,
            udp_rate_bps: None,
            duration_secs: 10,
            omit_secs: 1,
            phase: lantern_throughput::TestPhase::Single,
        };
        let operation = HelperOperation::ThroughputMeasurement {
            spec,
            limits: ThroughputLimits::from(&lantern_throughput::SuiteConfig::default()),
        };
        let request = AuthorizedRequest {
            run_id: Uuid::new_v4(),
            reviewed_plan_hash: reviewed_hash(&operation).unwrap(),
            operation,
        };

        assert!(matches!(
            request.validate(),
            Err(HelperError::Validation(_))
        ));
    }
    #[test]
    fn throughput_limits_are_validated_and_bound_to_the_reviewed_request() {
        let config = lantern_throughput::SuiteConfig {
            target: "fixture.invalid".into(),
            single_test: true,
            connect_timeout_ms: 1234,
            control_timeout_ms: 4321,
            max_control_json_bytes: 8192,
            ..Default::default()
        };
        let plan = lantern_throughput::plan(
            &config,
            lantern_throughput::ServerCapabilities {
                bidirectional: true,
            },
        )
        .unwrap();
        let spec = plan.tests().next().unwrap();
        let operation = HelperOperation::ThroughputMeasurement {
            spec,
            limits: ThroughputLimits::from(&config),
        };
        let mut request = AuthorizedRequest {
            run_id: Uuid::new_v4(),
            reviewed_plan_hash: reviewed_hash(&operation).unwrap(),
            operation,
        };
        request.validate().unwrap();
        let encoded = serde_json::to_vec(&request).unwrap();
        let decoded: AuthorizedRequest = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, request);
        if let HelperOperation::ThroughputMeasurement { limits, .. } = &mut request.operation {
            limits.control_timeout_ms = 4322;
        }
        assert!(matches!(request.validate(), Err(HelperError::Scope(_))));
        if let HelperOperation::ThroughputMeasurement { limits, .. } = &mut request.operation {
            limits.control_timeout_ms = 0;
        }
        assert!(matches!(
            request.validate(),
            Err(HelperError::Validation(_))
        ));
    }
}
