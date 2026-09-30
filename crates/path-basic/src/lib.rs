//! Native basic path diagnostics and side-effect-free plan construction.

use lantern_packet::AddressFamily;
use serde::{Deserialize, Serialize};
use std::{net::IpAddr, time::Duration};
use tokio_util::sync::CancellationToken;

pub use lantern_path_io::{
    HopResult, HopStatistics, ProbeError, RoundParameters, Sample, SampleStatus, Statistics, ping,
    resolve_target, tcp_trace, test_tcp, trace, udp_trace, validate_host,
};

pub const DEFAULT_IPV4_HOSTS: &[&str] = &[
    "cloudflare.com",
    "google.com",
    "wikipedia.org",
    "amazon.com",
];
pub const DEFAULT_IPV6_HOSTS: &[&str] = &["cloudflare.com", "google.com", "wikipedia.org"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BasicRound {
    Standard,
    #[serde(rename = "MTU1400_DF")]
    Mtu1400Df,
    #[serde(rename = "TTL64_Timeout5s")]
    Ttl64Timeout5s,
}

impl BasicRound {
    pub const ALL: [Self; 3] = [Self::Standard, Self::Mtu1400Df, Self::Ttl64Timeout5s];

    pub fn name(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Mtu1400Df => "MTU1400_DF",
            Self::Ttl64Timeout5s => "TTL64_Timeout5s",
        }
    }

    pub fn parameters(self, settings: &BasicSettings) -> RoundParameters {
        self.trace_parameters(settings)
    }

    fn ping_parameters(self, settings: &BasicSettings) -> RoundParameters {
        let mut parameters = self.trace_parameters(settings);
        if self == Self::Mtu1400Df {
            parameters.payload_size = 1400;
            parameters.dont_fragment = true;
        }
        parameters
    }

    fn trace_parameters(self, settings: &BasicSettings) -> RoundParameters {
        match self {
            Self::Standard | Self::Mtu1400Df => RoundParameters {
                payload_size: 32,
                dont_fragment: false,
                max_hops: settings.trace_max_hops,
                timeout: settings.trace_timeout,
                traffic_class: None,
            },
            Self::Ttl64Timeout5s => RoundParameters {
                payload_size: 32,
                dont_fragment: false,
                max_hops: 64,
                timeout: Duration::from_secs(5),
                traffic_class: None,
            },
        }
    }

    fn path_statistics_parameters(self, settings: &BasicSettings) -> RoundParameters {
        let mut parameters = self.trace_parameters(settings);
        parameters.timeout = match self {
            Self::Ttl64Timeout5s => Duration::from_secs(5),
            _ => settings.pathping_timeout,
        };
        parameters
    }

    fn ping_ttl(self) -> Option<u8> {
        (self == Self::Ttl64Timeout5s).then_some(64)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BasicSettings {
    pub ping_count: u16,
    pub trace_max_hops: u8,
    pub trace_timeout: Duration,
    pub pathping_probes: u16,
    pub pathping_timeout: Duration,
    pub tcp_port: u16,
    pub skip_pathping: bool,
}

impl Default for BasicSettings {
    fn default() -> Self {
        Self {
            ping_count: 5,
            trace_max_hops: 30,
            trace_timeout: Duration::from_secs(5),
            pathping_probes: 50,
            pathping_timeout: Duration::from_secs(3),
            tcp_port: 443,
            skip_pathping: false,
        }
    }
}

impl BasicSettings {
    pub fn validate(&self) -> Result<(), ProbeError> {
        if self.ping_count > 1000
            || self.pathping_probes > 1000
            || self.trace_timeout > Duration::from_secs(300)
            || self.pathping_timeout > Duration::from_secs(300)
        {
            return Err(ProbeError::InvalidPlan(
                "basic counts or timeouts exceed supported bounds".into(),
            ));
        }
        if self.ping_count == 0 {
            return Err(ProbeError::InvalidPlan(
                "ping count must be positive".into(),
            ));
        }
        if self.trace_max_hops == 0 {
            return Err(ProbeError::InvalidPlan(
                "trace max hops must be positive".into(),
            ));
        }
        if self.trace_timeout.is_zero() {
            return Err(ProbeError::InvalidPlan(
                "trace timeout must be positive".into(),
            ));
        }
        if !self.skip_pathping && self.pathping_probes == 0 {
            return Err(ProbeError::InvalidPlan(
                "path statistics probe count must be positive".into(),
            ));
        }
        if !self.skip_pathping && self.pathping_timeout.is_zero() {
            return Err(ProbeError::InvalidPlan(
                "path statistics timeout must be positive".into(),
            ));
        }
        if self.tcp_port == 0 {
            return Err(ProbeError::InvalidPlan("TCP port must be positive".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BasicPlanItem {
    pub round: BasicRound,
    pub family: AddressFamily,
    pub host: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BasicPlan {
    pub rounds: Vec<BasicRound>,
    pub families: Vec<AddressFamily>,
    pub ipv4_hosts: Vec<String>,
    pub ipv6_hosts: Vec<String>,
    pub total_items: u64,
}

impl BasicPlan {
    pub fn items(&self) -> impl Iterator<Item = BasicPlanItem> + '_ {
        self.rounds.iter().copied().flat_map(move |round| {
            self.families.iter().copied().flat_map(move |family| {
                let hosts = match family {
                    AddressFamily::Ipv4 => &self.ipv4_hosts,
                    AddressFamily::Ipv6 => &self.ipv6_hosts,
                };
                hosts.iter().cloned().map(move |host| BasicPlanItem {
                    round,
                    family,
                    host,
                })
            })
        })
    }
}

/// Expand rounds x address families x hosts without DNS or socket access.
pub fn build_plan(
    rounds: &[BasicRound],
    families: &[AddressFamily],
    ipv4_hosts: &[String],
    ipv6_hosts: &[String],
) -> Result<BasicPlan, ProbeError> {
    if rounds.is_empty() || families.is_empty() {
        return Err(ProbeError::InvalidPlan(
            "rounds and families must not be empty".into(),
        ));
    }
    for host in ipv4_hosts.iter().chain(ipv6_hosts) {
        validate_host(host)?;
    }
    let total_items = rounds.iter().try_fold(0u64, |total, _| {
        families.iter().try_fold(total, |total, family| {
            let hosts = match family {
                AddressFamily::Ipv4 => ipv4_hosts.len(),
                AddressFamily::Ipv6 => ipv6_hosts.len(),
            };
            total.checked_add(hosts as u64)
        })
    });
    let Some(total_items) = total_items else {
        return Err(ProbeError::InvalidPlan(
            "diagnostic plan cardinality overflow".into(),
        ));
    };
    if total_items == 0 {
        return Err(ProbeError::InvalidPlan("no diagnostic runs planned".into()));
    }
    Ok(BasicPlan {
        rounds: rounds.to_vec(),
        families: families.to_vec(),
        ipv4_hosts: ipv4_hosts.to_vec(),
        ipv6_hosts: ipv6_hosts.to_vec(),
        total_items,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BasicRunStatus {
    Completed,
    PartialFailure,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BasicStageError {
    pub stage: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BasicRunResult {
    pub round: BasicRound,
    pub family: AddressFamily,
    pub host: String,
    pub target: IpAddr,
    pub ping: Vec<Sample>,
    pub trace: Vec<HopResult>,
    pub path_statistics: Option<Vec<HopStatistics>>,
    pub tcp_443_ok: bool,
    pub failed_stages: Vec<String>,
    pub status: BasicRunStatus,
    pub stage_errors: Vec<BasicStageError>,
}

pub async fn execute_item(
    item: &BasicPlanItem,
    settings: &BasicSettings,
    cancellation: &CancellationToken,
) -> Result<BasicRunResult, ProbeError> {
    settings.validate()?;
    if cancellation.is_cancelled() {
        return Err(ProbeError::Cancelled);
    }
    let target =
        lantern_path_io::resolve_target_scoped(&item.host, item.family, cancellation).await?;
    let mut ping_samples = Vec::new();
    let mut trace_hops = Vec::new();
    let mut path_statistics = None;
    let mut tcp_443_ok = false;
    let mut failed_stages = Vec::new();
    let mut stage_errors = Vec::new();
    let mut cancelled = false;

    let ping_target = target;
    let ping_count = settings.ping_count;
    let ping_parameters = item.round.ping_parameters(settings);
    let ping_ttl = item.round.ping_ttl();
    match run_blocking_probe(cancellation, "ping task", move |token| {
        lantern_path_io::ping_scoped_with_ttl(
            ping_target,
            ping_count,
            ping_ttl,
            &ping_parameters,
            &token,
        )
    })
    .await
    {
        Ok(samples) => ping_samples = samples,
        Err(error) => record_stage_error(
            "Ping",
            error,
            &mut failed_stages,
            &mut stage_errors,
            &mut cancelled,
        ),
    }

    cancelled |= cancellation.is_cancelled();
    if !cancelled {
        let trace_parameters = item.round.trace_parameters(settings);
        match run_blocking_probe(cancellation, "trace task", move |token| {
            lantern_path_io::trace_scoped(target, 1, &trace_parameters, &token)
        })
        .await
        {
            Ok(hops) => trace_hops = hops,
            Err(error) => record_stage_error(
                "Tracert",
                error,
                &mut failed_stages,
                &mut stage_errors,
                &mut cancelled,
            ),
        }
    }

    cancelled |= cancellation.is_cancelled();
    if !cancelled && !settings.skip_pathping {
        let probes = settings.pathping_probes;
        let timeout = item.round.path_statistics_parameters(settings).timeout;
        let trace_copy = trace_hops.clone();
        let statistics_parameters = item.round.path_statistics_parameters(settings);
        match run_blocking_probe(cancellation, "path statistics task", move |token| {
            lantern_path_io::measure_hops_scoped(
                target,
                &trace_copy,
                probes,
                timeout,
                &statistics_parameters,
                &token,
            )
        })
        .await
        {
            Ok(statistics) => path_statistics = Some(statistics),
            Err(error) => record_stage_error(
                "Pathping",
                error,
                &mut failed_stages,
                &mut stage_errors,
                &mut cancelled,
            ),
        }
    }

    cancelled |= cancellation.is_cancelled();
    if !cancelled {
        let mut tcp_target = target;
        tcp_target.set_port(settings.tcp_port);
        match lantern_path_io::test_tcp_scoped(tcp_target, Duration::from_secs(5), cancellation)
            .await
        {
            Ok(ok) => tcp_443_ok = ok,
            Err(error) => record_stage_error(
                "Tcp443",
                error,
                &mut failed_stages,
                &mut stage_errors,
                &mut cancelled,
            ),
        }
    }
    cancelled |= cancellation.is_cancelled();
    if !cancelled {
        if ping_samples
            .iter()
            .all(|sample| sample.status != SampleStatus::Reply)
            && !failed_stages.iter().any(|stage| stage == "Ping")
        {
            record_stage_outcome(
                "Ping",
                "no ICMP echo reply was received",
                &mut failed_stages,
                &mut stage_errors,
            );
        }
        if trace_hops.last().is_none_or(|hop| !hop.reached_destination)
            && !failed_stages.iter().any(|stage| stage == "Tracert")
        {
            record_stage_outcome(
                "Tracert",
                "the destination was not reached",
                &mut failed_stages,
                &mut stage_errors,
            );
        }
        if path_statistics
            .as_ref()
            .is_some_and(|hops| hops.iter().all(|hop| hop.statistics.received == 0))
            && !failed_stages.iter().any(|stage| stage == "Pathping")
        {
            record_stage_outcome(
                "Pathping",
                "no path statistics reply was received",
                &mut failed_stages,
                &mut stage_errors,
            );
        }
        if !tcp_443_ok && !failed_stages.iter().any(|stage| stage == "Tcp443") {
            record_stage_outcome(
                "Tcp443",
                "the TCP connection did not succeed",
                &mut failed_stages,
                &mut stage_errors,
            );
        }
    }
    let status = if cancelled || cancellation.is_cancelled() {
        BasicRunStatus::Cancelled
    } else if failed_stages.is_empty() {
        BasicRunStatus::Completed
    } else {
        BasicRunStatus::PartialFailure
    };
    Ok(BasicRunResult {
        round: item.round,
        family: item.family,
        host: item.host.clone(),
        target: target.ip(),
        ping: ping_samples,
        trace: trace_hops,
        path_statistics,
        tcp_443_ok,
        failed_stages,
        status,
        stage_errors,
    })
}

async fn run_blocking_probe<T, F>(
    cancellation: &CancellationToken,
    operation: &'static str,
    probe: F,
) -> Result<T, ProbeError>
where
    T: Send + 'static,
    F: FnOnce(CancellationToken) -> Result<T, ProbeError> + Send + 'static,
{
    let child = cancellation.child_token();
    let guard = child.clone().drop_guard();
    let result = tokio::task::spawn_blocking(move || probe(child))
        .await
        .map_err(|error| ProbeError::Io {
            operation,
            message: error.to_string(),
        })?;
    drop(guard);
    result
}

fn record_stage_error(
    stage: &str,
    error: ProbeError,
    failed_stages: &mut Vec<String>,
    stage_errors: &mut Vec<BasicStageError>,
    cancelled: &mut bool,
) {
    if matches!(error, ProbeError::Cancelled) {
        *cancelled = true;
    }
    failed_stages.push(stage.into());
    stage_errors.push(BasicStageError {
        stage: stage.into(),
        message: error.to_string(),
    });
}

fn record_stage_outcome(
    stage: &str,
    message: &str,
    failed_stages: &mut Vec<String>,
    stage_errors: &mut Vec<BasicStageError>,
) {
    failed_stages.push(stage.into());
    stage_errors.push(BasicStageError {
        stage: stage.into(),
        message: message.into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    #[test]
    fn pure_plan_preserves_round_family_host_order() {
        let plan = build_plan(
            &[BasicRound::Standard, BasicRound::Mtu1400Df],
            &[AddressFamily::Ipv4, AddressFamily::Ipv6],
            &["v4.example".into()],
            &["v6.example".into()],
        )
        .unwrap();
        assert_eq!(plan.total_items, 4);
        let items = plan.items().collect::<Vec<_>>();
        assert_eq!(items[2].round, BasicRound::Mtu1400Df);
        assert_eq!(items[2].family, AddressFamily::Ipv4);
    }

    #[test]
    fn defaults_match_legacy_basic_contract() {
        let settings = BasicSettings::default();
        assert_eq!(settings.ping_count, 5);
        assert_eq!(settings.trace_max_hops, 30);
        assert_eq!(settings.pathping_probes, 50);
        assert_eq!(settings.pathping_timeout, Duration::from_secs(3));
        assert_eq!(
            BasicRound::ALL
                .iter()
                .map(|round| round.name())
                .collect::<Vec<_>>(),
            ["Standard", "MTU1400_DF", "TTL64_Timeout5s"]
        );
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn round_overrides_are_stage_specific() {
        let settings = BasicSettings {
            trace_timeout: Duration::from_secs(2),
            pathping_timeout: Duration::from_secs(3),
            ..BasicSettings::default()
        };
        let mtu_ping = BasicRound::Mtu1400Df.ping_parameters(&settings);
        let mtu_trace = BasicRound::Mtu1400Df.trace_parameters(&settings);
        assert_eq!(mtu_ping.payload_size, 1400);
        assert!(mtu_ping.dont_fragment);
        assert_eq!(mtu_trace.payload_size, 32);
        assert!(!mtu_trace.dont_fragment);
        let standard_ping = BasicRound::Standard.ping_parameters(&settings);
        assert_eq!(standard_ping.timeout, Duration::from_secs(2));
        assert_eq!(BasicRound::Standard.ping_ttl(), None);
        let slow = BasicRound::Ttl64Timeout5s;
        assert_eq!(slow.ping_ttl(), Some(64));
        assert_eq!(slow.trace_parameters(&settings).max_hops, 64);
        assert_eq!(
            slow.path_statistics_parameters(&settings).timeout,
            Duration::from_secs(5)
        );
    }

    #[test]
    fn computes_population_statistics_and_loss() {
        let samples = vec![
            Sample {
                sequence: 1,
                hop: None,
                elapsed_ms: Some(10.0),
                status: SampleStatus::Reply,
                next_hop_mtu: None,
                mpls: Vec::new(),
            },
            Sample {
                sequence: 2,
                hop: None,
                elapsed_ms: None,
                status: SampleStatus::Timeout,
                next_hop_mtu: None,
                mpls: Vec::new(),
            },
            Sample {
                sequence: 3,
                hop: None,
                elapsed_ms: Some(20.0),
                status: SampleStatus::Reply,
                next_hop_mtu: None,
                mpls: Vec::new(),
            },
        ];
        let stats = Statistics::from_samples(&samples);
        assert_eq!(stats.loss_percent, 100.0 / 3.0);
        assert_eq!(stats.average_ms, Some(15.0));
        assert_eq!(stats.stddev_ms, Some(5.0));
    }

    #[tokio::test]
    async fn dropping_blocking_probe_future_cancels_its_child_token() {
        let cancellation = CancellationToken::new();
        let stopped = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&stopped);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            run_blocking_probe(&cancellation, "test probe", move |token| {
                let _ = started_tx.send(());
                while !token.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(1));
                }
                observed.store(true, Ordering::SeqCst);
                Err::<(), _>(ProbeError::Cancelled)
            })
            .await
        });
        started_rx.await.expect("blocking probe did not start");
        task.abort();
        let _ = task.await;
        tokio::time::timeout(Duration::from_secs(1), async {
            while !stopped.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("blocking probe did not observe cancellation after future drop");
    }
}
