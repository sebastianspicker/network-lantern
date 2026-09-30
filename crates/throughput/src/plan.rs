use std::collections::HashSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::config::{Direction, DscpClass, Protocol, ServerCapabilities, SuiteConfig};
use crate::error::{Result, ThroughputError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestPhase {
    TcpMatrix,
    UdpFixed,
    UdpSaturation,
    Single,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestSpec {
    pub id: u64,
    pub target: String,
    pub port: u16,
    pub ip_version: crate::IpVersion,
    pub protocol: Protocol,
    pub direction: Direction,
    pub dscp: DscpClass,
    pub tos: u8,
    pub streams: u16,
    pub tcp_window_bytes: Option<u32>,
    pub udp_rate_bps: Option<u64>,
    pub duration_secs: u64,
    pub omit_secs: u64,
    pub phase: TestPhase,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThroughputPlan {
    pub config: SuiteConfig,
    pub capabilities: ServerCapabilities,
    pub total_tests: u64,
    pub estimated_test_seconds: u64,
    pub max_total_tests: Option<u64>,
    pub within_budget: bool,
}

impl ThroughputPlan {
    pub fn tests(&self) -> TestPlanIter<'_> {
        TestPlanIter {
            plan: self,
            next_id: 1,
            dscp: 0,
            stage: 0,
            direction: 0,
            stream: 0,
            window: 0,
            saturation_step: 0,
            emitted_single: false,
        }
    }
}

pub fn plan(config: &SuiteConfig, capabilities: ServerCapabilities) -> Result<ThroughputPlan> {
    validate(config)?;
    let dscp_count = if config.single_test {
        1
    } else {
        config.dscp_classes.len() as u64
    };
    let direction_count = if capabilities.bidirectional { 3 } else { 2 };
    let tcp_per_dscp = if config.protocol == Protocol::Udp || config.single_test {
        0
    } else {
        direction_count * config.tcp_streams.len() as u64 * config.tcp_windows_bytes.len() as u64
    };
    let saturation_steps = if config.protocol == Protocol::Tcp
        || config.single_test
        || config.udp_max_bps <= config.udp_start_bps
    {
        0
    } else {
        ((config.udp_max_bps - config.udp_start_bps) / config.udp_step_bps + 1).min(1000)
    };
    let udp_per_dscp = if config.protocol == Protocol::Tcp || config.single_test {
        0
    } else {
        2 + 2 * saturation_steps
    };
    let total_tests = if config.single_test {
        1
    } else {
        dscp_count * (tcp_per_dscp + udp_per_dscp)
    };
    if let Some(maximum) = config.max_total_tests.filter(|maximum| *maximum != 0)
        && total_tests > maximum
    {
        return Err(ThroughputError::BudgetExceeded {
            planned: total_tests,
            maximum,
        });
    }
    Ok(ThroughputPlan {
        config: config.clone(),
        capabilities,
        total_tests,
        estimated_test_seconds: total_tests
            .saturating_mul(config.duration_secs.saturating_add(config.omit_secs)),
        max_total_tests: config.max_total_tests.filter(|maximum| *maximum != 0),
        within_budget: true,
    })
}

fn validate(config: &SuiteConfig) -> Result<()> {
    if config.target.is_empty()
        || config.target.len() > 253
        || config.target.chars().any(|c| {
            c.is_whitespace()
                || c.is_control()
                || matches!(c, '/' | '\\' | ';' | '&' | '|' | '`' | '$')
        })
    {
        return Err(ThroughputError::Validation(
            "target must be a valid hostname or IP address up to 253 bytes".into(),
        ));
    }
    if !(1..=3600).contains(&config.duration_secs) {
        return Err(ThroughputError::Validation(
            "duration_secs must be between 1 and 3600".into(),
        ));
    }
    if config.max_total_tests.is_some_and(|n| n > 1_000_000) {
        return Err(ThroughputError::Validation(
            "max_total_tests must be in 0..=1000000".into(),
        ));
    }
    if config.omit_secs > 60 {
        return Err(ThroughputError::Validation(
            "omit_secs must be at most 60".into(),
        ));
    }
    if config.mtu_sizes.is_empty()
        || config.mtu_sizes.len() > 128
        || config.mtu_sizes.iter().any(|n| *n == 0 || *n > 65500)
    {
        return Err(ThroughputError::Validation(
            "mtu_sizes must contain 1–128 payload sizes in 1..=65500".into(),
        ));
    }
    if config.dscp_classes.is_empty() {
        return Err(ThroughputError::Validation(
            "at least one DSCP class is required".into(),
        ));
    }
    if config.tcp_streams.is_empty()
        || config.tcp_streams.len() > 128
        || config.tcp_streams.iter().any(|n| !(1..=128).contains(n))
    {
        return Err(ThroughputError::Validation(
            "tcp_streams must contain values between 1 and 128".into(),
        ));
    }
    if config.tcp_windows_bytes.is_empty() || config.tcp_windows_bytes.len() > 128 {
        return Err(ThroughputError::Validation(
            "tcp_windows_bytes must not be empty".into(),
        ));
    }
    if config.udp_start_bps == 0 || config.udp_step_bps == 0 {
        return Err(ThroughputError::Validation(
            "UDP start and step rates must be greater than zero".into(),
        ));
    }
    if config.udp_start_bps > crate::MAX_UDP_RATE_BPS
        || config.udp_max_bps > crate::MAX_UDP_RATE_BPS
        || config.udp_step_bps > crate::MAX_UDP_RATE_BPS
    {
        return Err(ThroughputError::Validation(format!(
            "UDP rates must not exceed {} bits per second",
            crate::MAX_UDP_RATE_BPS
        )));
    }
    if !config.udp_loss_threshold_pct.is_finite()
        || !(0.0..=100.0).contains(&config.udp_loss_threshold_pct)
    {
        return Err(ThroughputError::Validation(
            "udp_loss_threshold_pct must be finite and between 0 and 100".into(),
        ));
    }
    if config.retry.max_retries > 5 {
        return Err(ThroughputError::Validation(
            "retry.max_retries must be at most 5".into(),
        ));
    }
    if config.dscp_classes.len() > 21 {
        return Err(ThroughputError::Validation(
            "dscp_classes contains more values than the 21 supported classes".into(),
        ));
    }
    if !(1..=crate::protocol::MAX_CLIENT_TIMEOUT_MS).contains(&config.connect_timeout_ms)
        || !(1..=crate::protocol::MAX_CLIENT_TIMEOUT_MS).contains(&config.control_timeout_ms)
        || !(2..=1024 * 1024).contains(&config.max_control_json_bytes)
    {
        return Err(ThroughputError::Validation(format!(
            "timeouts must be between 1 and {} ms and max_control_json_bytes between 2 and 1048576",
            crate::protocol::MAX_CLIENT_TIMEOUT_MS
        )));
    }
    Ok(())
}

pub struct TestPlanIter<'a> {
    plan: &'a ThroughputPlan,
    next_id: u64,
    dscp: usize,
    stage: u8,
    direction: usize,
    stream: usize,
    window: usize,
    saturation_step: u64,
    emitted_single: bool,
}

struct PlanCase {
    protocol: Protocol,
    direction: Direction,
    dscp: DscpClass,
    streams: u16,
    window: Option<u32>,
    udp_rate: Option<u64>,
    phase: TestPhase,
}

impl TestPlanIter<'_> {
    fn spec(&mut self, case: PlanCase) -> TestSpec {
        let id = self.next_id;
        self.next_id += 1;
        TestSpec {
            id,
            target: self.plan.config.target.clone(),
            port: self.plan.config.port,
            ip_version: self.plan.config.ip_version,
            protocol: case.protocol,
            direction: case.direction,
            dscp: case.dscp,
            tos: case.dscp.tos(),
            streams: case.streams,
            tcp_window_bytes: case.window,
            udp_rate_bps: case.udp_rate,
            duration_secs: self.plan.config.duration_secs,
            omit_secs: self.plan.config.omit_secs,
            phase: case.phase,
        }
    }
}

impl Iterator for TestPlanIter<'_> {
    type Item = TestSpec;

    fn next(&mut self) -> Option<Self::Item> {
        let cfg = &self.plan.config;
        if cfg.single_test {
            if self.emitted_single {
                return None;
            }
            self.emitted_single = true;
            let protocol = if cfg.protocol == Protocol::Udp {
                Protocol::Udp
            } else {
                Protocol::Tcp
            };
            return Some(self.spec(PlanCase {
                protocol,
                direction: Direction::Tx,
                dscp: cfg.dscp_classes[0],
                streams: 1,
                window: None,
                udp_rate: (protocol == Protocol::Udp).then_some(cfg.udp_start_bps),
                phase: TestPhase::Single,
            }));
        }
        while self.dscp < cfg.dscp_classes.len() {
            let dscp = cfg.dscp_classes[self.dscp];
            if self.stage == 0 && cfg.protocol != Protocol::Udp {
                let directions: &[Direction] = if self.plan.capabilities.bidirectional {
                    &[Direction::Tx, Direction::Rx, Direction::Bidirectional]
                } else {
                    &[Direction::Tx, Direction::Rx]
                };
                if self.direction < directions.len() {
                    let result = self.spec(PlanCase {
                        protocol: Protocol::Tcp,
                        direction: directions[self.direction],
                        dscp,
                        streams: cfg.tcp_streams[self.stream],
                        window: cfg.tcp_windows_bytes[self.window],
                        udp_rate: None,
                        phase: TestPhase::TcpMatrix,
                    });
                    self.window += 1;
                    if self.window == cfg.tcp_windows_bytes.len() {
                        self.window = 0;
                        self.stream += 1;
                        if self.stream == cfg.tcp_streams.len() {
                            self.stream = 0;
                            self.direction += 1;
                        }
                    }
                    return Some(result);
                }
            }
            if self.stage == 0 {
                self.stage = 1;
                self.direction = 0;
            }
            if self.stage == 1 && cfg.protocol != Protocol::Tcp && self.direction < 2 {
                let dir = [Direction::Tx, Direction::Rx][self.direction];
                self.direction += 1;
                return Some(self.spec(PlanCase {
                    protocol: Protocol::Udp,
                    direction: dir,
                    dscp,
                    streams: 1,
                    window: None,
                    udp_rate: Some(cfg.udp_start_bps),
                    phase: TestPhase::UdpFixed,
                }));
            }
            if self.stage == 1 {
                self.stage = 2;
                self.direction = 0;
            }
            if cfg.protocol != Protocol::Tcp && cfg.udp_max_bps > cfg.udp_start_bps {
                let steps =
                    ((cfg.udp_max_bps - cfg.udp_start_bps) / cfg.udp_step_bps + 1).min(1000);
                if self.saturation_step < steps {
                    let dir = [Direction::Tx, Direction::Rx][self.direction];
                    let rate = cfg
                        .udp_start_bps
                        .saturating_add(cfg.udp_step_bps.saturating_mul(self.saturation_step));
                    self.direction += 1;
                    if self.direction == 2 {
                        self.direction = 0;
                        self.saturation_step += 1;
                    }
                    return Some(self.spec(PlanCase {
                        protocol: Protocol::Udp,
                        direction: dir,
                        dscp,
                        streams: 1,
                        window: None,
                        udp_rate: Some(rate),
                        phase: TestPhase::UdpSaturation,
                    }));
                }
            }
            self.dscp += 1;
            self.stage = 0;
            self.direction = 0;
            self.saturation_step = 0;
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.plan.total_tests.saturating_sub(self.next_id - 1);
        let count = usize::try_from(remaining).unwrap_or(usize::MAX);
        (count, Some(count))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestResult {
    pub spec: TestSpec,
    pub attempts: u8,
    pub elapsed_ms: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub tx_mbps: Option<f64>,
    pub rx_mbps: Option<f64>,
    pub retransmits: Option<u64>,
    pub loss_pct: Option<f64>,
    pub loss_scope: Option<String>,
    pub jitter_ms: Option<f64>,
    pub server_result: String,
    pub server_result_truncated: bool,
    pub unavailable_metrics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanRun {
    pub planned_tests: u64,
    pub executed_tests: u64,
    pub saturation_tests_skipped: u64,
    pub results: Vec<TestResult>,
}

impl crate::Iperf3Client {
    pub async fn run_plan(
        &self,
        plan: &ThroughputPlan,
        cancellation: &CancellationToken,
    ) -> Result<PlanRun> {
        let mut results = Vec::new();
        let mut saturated = HashSet::new();
        let mut skipped = 0;
        for spec in plan.tests() {
            if spec.phase == TestPhase::UdpSaturation
                && saturated.contains(&(spec.dscp, spec.direction))
            {
                skipped += 1;
                continue;
            }
            let result = self
                .run_test_with_retry(&spec, plan.config.retry, cancellation)
                .await?;
            if spec.phase == TestPhase::UdpSaturation
                && result
                    .loss_pct
                    .is_some_and(|loss| loss >= plan.config.udp_loss_threshold_pct)
            {
                saturated.insert((spec.dscp, spec.direction));
            }
            results.push(result);
        }
        Ok(PlanRun {
            planned_tests: plan.total_tests,
            executed_tests: results.len() as u64,
            saturation_tests_skipped: skipped,
            results,
        })
    }

    pub(crate) async fn retry_backoff(
        cancellation: &CancellationToken,
        delay_ms: u64,
    ) -> Result<()> {
        tokio::select! {
            _ = cancellation.cancelled() => Err(ThroughputError::Cancelled),
            _ = tokio::time::sleep(Duration::from_millis(delay_ms)) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> SuiteConfig {
        SuiteConfig {
            target: "127.0.0.1".into(),
            ..SuiteConfig::default()
        }
    }

    #[test]
    fn default_counts_match_existing_contract() {
        let bidir = plan(
            &defaults(),
            ServerCapabilities {
                bidirectional: true,
            },
        )
        .unwrap();
        let one_way = plan(
            &defaults(),
            ServerCapabilities {
                bidirectional: false,
            },
        )
        .unwrap();
        assert_eq!(bidir.total_tests, 1145);
        assert_eq!(one_way.total_tests, 1100);
        assert_eq!(bidir.tests().count() as u64, bidir.total_tests);
        assert_eq!(one_way.tests().count() as u64, one_way.total_tests);
    }

    #[test]
    fn fractional_udp_steps_match_legacy_floor_plus_one() {
        let config = SuiteConfig {
            target: "127.0.0.1".into(),
            protocol: Protocol::Udp,
            dscp_classes: vec![DscpClass::CS0],
            udp_start_bps: 1_000_000,
            udp_max_bps: 2_500_000,
            udp_step_bps: 1_000_000,
            ..SuiteConfig::default()
        };
        let plan = plan(&config, ServerCapabilities::default()).unwrap();
        assert_eq!(plan.total_tests, 6);
        assert_eq!(plan.tests().last().unwrap().udp_rate_bps, Some(2_000_000));
    }

    #[test]
    fn zero_budget_is_unlimited_and_saturation_is_bounded() {
        let config = SuiteConfig {
            max_total_tests: Some(0),
            protocol: Protocol::Udp,
            dscp_classes: vec![DscpClass::CS0],
            udp_start_bps: 1,
            udp_max_bps: crate::MAX_UDP_RATE_BPS,
            udp_step_bps: 1,
            ..defaults()
        };
        let plan = plan(&config, ServerCapabilities::default()).unwrap();
        assert_eq!(plan.max_total_tests, None);
        assert_eq!(plan.total_tests, 2002);
        assert_eq!(plan.tests().count(), 2002);
    }

    #[test]
    fn excessive_udp_rate_is_rejected_before_network_io() {
        let config = SuiteConfig {
            protocol: Protocol::Udp,
            udp_max_bps: crate::MAX_UDP_RATE_BPS + 1,
            ..defaults()
        };
        assert!(matches!(
            plan(&config, ServerCapabilities::default()),
            Err(ThroughputError::Validation(message)) if message.contains("UDP rates")
        ));
    }

    #[test]
    fn excessive_timeouts_are_rejected_during_pure_planning() {
        for config in [
            SuiteConfig {
                connect_timeout_ms: crate::protocol::MAX_CLIENT_TIMEOUT_MS + 1,
                ..defaults()
            },
            SuiteConfig {
                control_timeout_ms: crate::protocol::MAX_CLIENT_TIMEOUT_MS + 1,
                ..defaults()
            },
        ] {
            assert!(matches!(
                plan(&config, ServerCapabilities::default()),
                Err(ThroughputError::Validation(message)) if message.contains("timeouts")
            ));
        }
    }

    #[tokio::test]
    async fn cancelled_retry_backoff_returns_promptly() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert!(matches!(
            crate::Iperf3Client::retry_backoff(&cancellation, 60_000).await,
            Err(ThroughputError::Cancelled)
        ));
    }

    #[test]
    fn budget_fails_during_pure_planning() {
        let config = SuiteConfig {
            max_total_tests: Some(1),
            ..defaults()
        };
        assert!(matches!(
            plan(&config, ServerCapabilities::default()),
            Err(ThroughputError::BudgetExceeded {
                planned: 1145,
                maximum: 1
            })
        ));
    }
}
