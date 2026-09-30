//! MTR-style native path trace planning and execution.

mod aggregate;
mod cadence;
use cadence::Cadence;
mod dns;
use aggregate::HopAccumulator;
#[cfg(test)]
use lantern_path_io::Sample;

use lantern_packet::AddressFamily;
use lantern_path_io::{self as path_io, ProbeError, RoundParameters, Statistics};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

pub const DEFAULT_IPV4_HOSTS: &[&str] = &[
    "cloudflare.com",
    "google.com",
    "wikipedia.org",
    "amazon.com",
];
pub const DEFAULT_IPV6_HOSTS: &[&str] = &["cloudflare.com", "google.com", "wikipedia.org"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceType {
    #[serde(rename = "ICMP4")]
    Icmp4,
    #[serde(rename = "ICMP6")]
    Icmp6,
    #[serde(rename = "UDP4")]
    Udp4,
    #[serde(rename = "UDP6")]
    Udp6,
    #[serde(rename = "TCP4")]
    Tcp4,
    #[serde(rename = "TCP6")]
    Tcp6,
    #[serde(rename = "MPLS4")]
    Mpls4,
    #[serde(rename = "MPLS6")]
    Mpls6,
    #[serde(rename = "AS4")]
    As4,
    #[serde(rename = "AS6")]
    As6,
}

impl TraceType {
    pub const ALL: [Self; 10] = [
        Self::Icmp4,
        Self::Icmp6,
        Self::Udp4,
        Self::Udp6,
        Self::Tcp4,
        Self::Tcp6,
        Self::Mpls4,
        Self::Mpls6,
        Self::As4,
        Self::As6,
    ];
    pub const DEFAULTS: [Self; 4] = [Self::Icmp4, Self::Icmp6, Self::Tcp4, Self::Tcp6];
    pub fn family(self) -> AddressFamily {
        match self {
            Self::Icmp4 | Self::Udp4 | Self::Tcp4 | Self::Mpls4 | Self::As4 => AddressFamily::Ipv4,
            _ => AddressFamily::Ipv6,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Icmp4 => "ICMP4",
            Self::Icmp6 => "ICMP6",
            Self::Udp4 => "UDP4",
            Self::Udp6 => "UDP6",
            Self::Tcp4 => "TCP4",
            Self::Tcp6 => "TCP6",
            Self::Mpls4 => "MPLS4",
            Self::Mpls6 => "MPLS6",
            Self::As4 => "AS4",
            Self::As6 => "AS6",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceRound {
    Standard,
    #[serde(rename = "MTU1400")]
    Mtu1400,
    #[serde(rename = "TOS_CS5")]
    TosCs5,
    #[serde(rename = "TOS_AF11")]
    TosAf11,
    #[serde(rename = "TTL10")]
    Ttl10,
    #[serde(rename = "TTL64")]
    Ttl64,
    #[serde(rename = "FirstTTL3")]
    FirstTtl3,
    #[serde(rename = "Timeout5")]
    Timeout5,
}

impl TraceRound {
    pub const ALL: [Self; 8] = [
        Self::Standard,
        Self::Mtu1400,
        Self::TosCs5,
        Self::TosAf11,
        Self::Ttl10,
        Self::Ttl64,
        Self::FirstTtl3,
        Self::Timeout5,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Mtu1400 => "MTU1400",
            Self::TosCs5 => "TOS_CS5",
            Self::TosAf11 => "TOS_AF11",
            Self::Ttl10 => "TTL10",
            Self::Ttl64 => "TTL64",
            Self::FirstTtl3 => "FirstTTL3",
            Self::Timeout5 => "Timeout5",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceSettings {
    pub cycles: u16,
    pub interval: Duration,
    pub run_timeout: Duration,
    pub probe_timeout: Duration,
    pub max_hops: u8,
    pub first_ttl: u8,
    /// Total IP packet size, matching MTR -s (headers included).
    pub packet_size: usize,
    pub tcp_port: u16,
    pub udp_base_port: u16,
    pub dns_resolver: Option<SocketAddr>,
    pub dns_timeout: Duration,
}

impl Default for TraceSettings {
    fn default() -> Self {
        Self {
            cycles: 300,
            interval: Duration::from_secs(1),
            run_timeout: Duration::from_secs(360),
            probe_timeout: Duration::from_secs(10),
            max_hops: 30,
            first_ttl: 1,
            packet_size: 64,
            tcp_port: 443,
            udp_base_port: 33434,
            dns_resolver: None,
            dns_timeout: Duration::from_secs(2),
        }
    }
}

impl TraceSettings {
    pub fn validate(&self) -> Result<(), ProbeError> {
        if self.cycles > 10000
            || self.interval > Duration::from_secs(60)
            || self.run_timeout > Duration::from_secs(86400)
            || self.probe_timeout > Duration::from_secs(300)
            || self.dns_timeout > Duration::from_secs(300)
            || self.packet_size > 65535
        {
            return Err(ProbeError::InvalidPlan(
                "trace counts, payload or timeouts exceed supported bounds".into(),
            ));
        }
        if self.cycles == 0 || self.max_hops == 0 || self.first_ttl == 0 {
            return Err(ProbeError::InvalidPlan(
                "cycles and hop limits must be positive".into(),
            ));
        }
        if self.first_ttl > self.max_hops {
            return Err(ProbeError::InvalidPlan(
                "first TTL must not exceed max hops".into(),
            ));
        }
        if self.interval.is_zero()
            || self.run_timeout.is_zero()
            || self.probe_timeout.is_zero()
            || self.dns_timeout.is_zero()
        {
            return Err(ProbeError::InvalidPlan(
                "trace intervals and timeouts must be positive".into(),
            ));
        }
        if self.packet_size < 64 || self.tcp_port == 0 || self.udp_base_port == 0 {
            return Err(ProbeError::InvalidPlan(
                "payload size and destination ports must be positive".into(),
            ));
        }
        Ok(())
    }

    pub fn validate_for_type(&self, probe_type: TraceType) -> Result<(), ProbeError> {
        self.validate()?;
        if !matches!(probe_type, TraceType::Udp4 | TraceType::Udp6) {
            return Ok(());
        }
        let available_ports = u32::from(u16::MAX - self.udp_base_port) + 1;
        let last_identity = u32::from(self.cycles - 1)
            .checked_mul(u32::from(self.max_hops) + 1)
            .and_then(|value| value.checked_add(u32::from(self.max_hops)))
            .ok_or_else(|| ProbeError::InvalidPlan("trace probe identity overflow".into()))?;
        if last_identity >= available_ports {
            return Err(ProbeError::InvalidPlan(
                "UDP destination port range cannot uniquely encode every planned probe".into(),
            ));
        }
        Ok(())
    }
}

impl TraceRound {
    pub fn apply(self, defaults: &TraceSettings) -> TraceSettings {
        let mut value = defaults.clone();
        match self {
            Self::Standard => {}
            Self::Mtu1400 => value.packet_size = 1400,
            Self::TosCs5 | Self::TosAf11 => {}
            Self::Ttl10 => value.max_hops = 10,
            Self::Ttl64 => value.max_hops = 64,
            Self::FirstTtl3 => value.first_ttl = 3,
            Self::Timeout5 => value.probe_timeout = Duration::from_secs(5),
        }
        value
    }
    pub fn traffic_class(self) -> Option<u8> {
        match self {
            Self::TosCs5 => Some(160),
            Self::TosAf11 => Some(40),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TracePlanItem {
    pub round: TraceRound,
    pub probe_type: TraceType,
    pub host: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TracePlan {
    pub rounds: Vec<TraceRound>,
    pub types: Vec<TraceType>,
    pub ipv4_hosts: Vec<String>,
    pub ipv6_hosts: Vec<String>,
    pub total_items: u64,
}

impl TracePlan {
    pub fn items(&self) -> impl Iterator<Item = TracePlanItem> + '_ {
        self.rounds.iter().copied().flat_map(move |round| {
            self.types.iter().copied().flat_map(move |probe_type| {
                let hosts = match probe_type.family() {
                    AddressFamily::Ipv4 => &self.ipv4_hosts,
                    AddressFamily::Ipv6 => &self.ipv6_hosts,
                };
                hosts.iter().cloned().map(move |host| TracePlanItem {
                    round,
                    probe_type,
                    host,
                })
            })
        })
    }
}

pub fn build_plan(
    rounds: &[TraceRound],
    types: &[TraceType],
    ipv4_hosts: &[String],
    ipv6_hosts: &[String],
) -> Result<TracePlan, ProbeError> {
    if rounds.is_empty() || types.is_empty() {
        return Err(ProbeError::InvalidPlan(
            "rounds and probe types must not be empty".into(),
        ));
    }
    for host in ipv4_hosts.iter().chain(ipv6_hosts) {
        path_io::validate_host(host)?;
    }
    let total_items = rounds.iter().try_fold(0u64, |total, _| {
        types.iter().try_fold(total, |total, probe_type| {
            let hosts = match probe_type.family() {
                AddressFamily::Ipv4 => ipv4_hosts.len(),
                AddressFamily::Ipv6 => ipv6_hosts.len(),
            };
            total.checked_add(hosts as u64)
        })
    });
    let Some(total_items) = total_items else {
        return Err(ProbeError::InvalidPlan(
            "trace plan cardinality overflow".into(),
        ));
    };
    if total_items == 0 {
        return Err(ProbeError::InvalidPlan("no trace runs planned".into()));
    }
    Ok(TracePlan {
        rounds: rounds.to_vec(),
        types: types.to_vec(),
        ipv4_hosts: ipv4_hosts.to_vec(),
        ipv6_hosts: ipv6_hosts.to_vec(),
        total_items,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceHop {
    pub ttl: u8,
    pub address: Option<IpAddr>,
    pub hostname: Option<String>,
    pub asn: Option<String>,
    pub mpls: Vec<lantern_packet::MplsLabel>,
    pub mpls_stacks: Vec<Vec<lantern_packet::MplsLabel>>,
    pub responders: Vec<TraceResponder>,
    pub statistics: Statistics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceResponder {
    pub address: IpAddr,
    pub statistics: Statistics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceRunStatus {
    Completed,
    PartialFailure,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceRunResult {
    pub round: TraceRound,
    pub probe_type: TraceType,
    pub host: String,
    pub target: IpAddr,
    pub hops: Vec<TraceHop>,
    pub status: TraceRunStatus,
    pub planned_cycles: u16,
    pub completed_cycles: u16,
    pub enrichment_complete: bool,
    pub errors: Vec<String>,
}

const MAX_OUTSTANDING_PROBES: usize = 512;
const MAX_DNS_QUERIES: usize = 8;
const FINAL_REPLY_GRACE: Duration = Duration::from_secs(5);
const PROBE_CLEANUP_GRACE: Duration = Duration::from_millis(250);
const MAX_ERROR_BYTES: usize = 16 * 1024;

pub async fn execute_run(
    item: &TracePlanItem,
    settings: &TraceSettings,
    cancellation: &CancellationToken,
) -> Result<TraceRunResult, ProbeError> {
    execute_run_with_probe(item, settings, cancellation, trace_probe).await
}

async fn execute_run_with_probe(
    item: &TracePlanItem,
    settings: &TraceSettings,
    cancellation: &CancellationToken,
    probe_fn: fn(
        &CycleProbe,
        u16,
        u8,
        &CancellationToken,
    ) -> Result<path_io::HopResult, ProbeError>,
) -> Result<TraceRunResult, ProbeError> {
    settings.validate_for_type(item.probe_type)?;
    if cancellation.is_cancelled() {
        return Err(ProbeError::Cancelled);
    }
    let configured = item.round.apply(settings);
    configured.validate_for_type(item.probe_type)?;
    let target =
        path_io::resolve_target_scoped(&item.host, item.probe_type.family(), cancellation).await?;
    let parameters = RoundParameters {
        payload_size: configured.packet_size
            - match item.probe_type.family() {
                AddressFamily::Ipv4 => 28,
                AddressFamily::Ipv6 => 48,
            },
        dont_fragment: false,
        max_hops: configured.max_hops,
        timeout: configured.probe_timeout,
        traffic_class: item.round.traffic_class(),
    };
    let mut samples_by_hop: Vec<HopAccumulator> = (0..=usize::from(configured.max_hops))
        .map(|_| HopAccumulator::default())
        .collect();
    let run_token = cancellation.child_token();
    let run_guard = run_token.clone().drop_guard();
    let probe_token = run_token.child_token();
    let started = tokio::time::Instant::now();
    let deadline = started + configured.run_timeout;
    let nonce = rand::random::<u32>();
    let mut tasks: JoinSet<(u16, u8, Result<path_io::HopResult, ProbeError>)> = JoinSet::new();
    let mut completed_cycles = 0u16;
    let mut pending = vec![0u16; usize::from(configured.cycles)];
    let mut destination_bound = None::<u8>;
    let mut known_hops = vec![false; usize::from(configured.max_hops) + 1];
    let mut errors = Vec::new();
    let mut status = TraceRunStatus::Completed;
    let mut final_grace_deadline = None;
    let mut normal_grace_expired = false;
    let mut cadence = Cadence::new(
        started,
        configured.interval,
        configured.first_ttl,
        configured.max_hops,
    );

    loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                status = TraceRunStatus::Cancelled;
                errors.push("trace run cancelled".into());
                run_token.cancel();
                break;
            }
            _ = tokio::time::sleep_until(deadline) => {
                status = TraceRunStatus::TimedOut;
                errors.push(format!("trace run timed out after {:?}", configured.run_timeout));
                run_token.cancel();
                break;
            }
            _ = tokio::time::sleep_until(final_grace_deadline.unwrap_or(deadline)), if final_grace_deadline.is_some() => {
                normal_grace_expired = true;
                probe_token.cancel();
                break;
            }
            result = tasks.join_next(), if !tasks.is_empty() => {
                if let Some(result) = result {
                    match result {
                        Ok((cycle, probe_ttl, result)) => {
                            pending[usize::from(cycle)] -= 1;
                            if cycle < cadence.cycles && pending[usize::from(cycle)] == 0 { completed_cycles += 1; }
                            match result {
                                Ok(hop) => {
                                    known_hops[usize::from(probe_ttl)] |= hop.address.is_some();
                                    if hop.reached_destination { destination_bound = Some(destination_bound.map_or(probe_ttl, |old| old.min(probe_ttl))); }
                                    for sample in hop.samples { samples_by_hop[usize::from(probe_ttl)].record(sample); }
                                }
                                Err(error) => {
                                    let fatal = matches!(error, ProbeError::Permission { .. } | ProbeError::Unsupported { .. });
                                    extend_errors(&mut errors, [format!("cycle {} TTL {probe_ttl}: {error}", cycle + 1)]);
                                    if fatal { status = TraceRunStatus::PartialFailure; run_token.cancel(); break; }
                                }
                            }
                        }
                        Err(error) => { extend_errors(&mut errors, [format!("trace probe task failed: {error}")]); status = TraceRunStatus::PartialFailure; run_token.cancel(); break; }
                    }
                }
            }
            _ = tokio::time::sleep_until(cadence.deadline), if cadence.cycles < configured.cycles => {
                if tasks.len() >= MAX_OUTSTANDING_PROBES {
                    errors.push("Native probe concurrency limit reached; remaining probes were not sent".into());
                    status = TraceRunStatus::PartialFailure;
                    run_token.cancel();
                    break;
                }
                let cycle = cadence.cycles;
                let sent_ttl = cadence.ttl;
                let probe = CycleProbe { target, probe_type: item.probe_type, configured: configured.clone(), parameters: parameters.clone(), nonce };
                let token = probe_token.clone();
                pending[usize::from(cycle)] += 1;
                tasks.spawn_blocking(move || (cycle, sent_ttl, probe_fn(&probe, cycle, sent_ttl, &token)));
                cadence.dispatched(tokio::time::Instant::now(), &known_hops, destination_bound);
                if cadence.cycles == configured.cycles {
                    final_grace_deadline = Some(final_reply_deadline(&cadence));
                }
            }
        }
    }
    // Collect completed samples from probes interrupted partway through a cycle.
    // The socket boundary observes cancellation; keep a finite shutdown deadline.
    let shutdown = tokio::time::Instant::now() + PROBE_CLEANUP_GRACE;
    while !tasks.is_empty() {
        match tokio::time::timeout_at(shutdown, tasks.join_next()).await {
            Ok(Some(Ok((cycle, probe_ttl, result)))) => {
                pending[usize::from(cycle)] -= 1;
                if cycle < cadence.cycles && pending[usize::from(cycle)] == 0 {
                    completed_cycles += 1;
                }
                match result {
                    Ok(hop) => {
                        for sample in hop
                            .samples
                            .into_iter()
                            .map(|sample| settle_sample(sample, normal_grace_expired))
                        {
                            samples_by_hop[usize::from(probe_ttl)].record(sample);
                        }
                    }
                    Err(ProbeError::Cancelled) => {}
                    Err(error) => extend_errors(
                        &mut errors,
                        [format!("cycle {} TTL {probe_ttl}: {error}", cycle + 1)],
                    ),
                }
            }
            Ok(Some(Err(error))) => {
                extend_errors(&mut errors, [format!("trace probe task failed: {error}")])
            }
            Ok(None) => break,
            Err(_) => {
                errors.push("Native probe shutdown deadline exceeded".into());
                tasks.abort_all();
                break;
            }
        }
    }
    if samples_by_hop.iter().any(|hop| hop.limited) {
        errors.push("Responder/MPLS retention reached the 64 entries per hop limit; aggregate statistics include all samples".into());
    }
    let mut hops: Vec<_> = samples_by_hop
        .into_iter()
        .enumerate()
        .skip(usize::from(configured.first_ttl))
        .filter_map(|(ttl, values)| values.finish(ttl as u8))
        .collect();
    let mut enrichment_complete = false;
    if status == TraceRunStatus::Completed {
        let needs_asn = matches!(item.probe_type, TraceType::As4 | TraceType::As6);
        match settings.dns_resolver.or_else(dns::system_resolver) {
            Some(resolver) => {
                let enrichment = enrich_hops(
                    &mut hops,
                    resolver,
                    settings.dns_timeout,
                    needs_asn,
                    &run_token,
                );
                tokio::select! {
                    _ = cancellation.cancelled() => {
                        status = TraceRunStatus::Cancelled;
                        errors.push("DNS enrichment cancelled".into());
                        run_token.cancel();
                    }
                    _ = tokio::time::sleep_until(deadline) => {
                        status = TraceRunStatus::TimedOut;
                        errors.push(format!("trace run timed out after {:?} during DNS enrichment", configured.run_timeout));
                        run_token.cancel();
                    }
                    enrichment_errors = enrichment => {
                        if cancellation.is_cancelled() {
                            status = TraceRunStatus::Cancelled;
                            errors.push("DNS enrichment cancelled".into());
                        } else {
                            enrichment_complete = enrichment_errors.is_empty();
                        }
                        extend_errors(&mut errors, enrichment_errors);
                    }
                }
            }
            None if needs_asn => errors
                .push("ASN DNS enrichment: no DNS resolver was configured or discoverable".into()),
            None => enrichment_complete = true,
        }
    }
    if status == TraceRunStatus::Completed
        && (hops.is_empty() || hops.iter().all(|hop| hop.statistics.received == 0))
    {
        errors.push("no trace probe responses were received".into());
    }
    if status == TraceRunStatus::Completed && !errors.is_empty() {
        status = TraceRunStatus::PartialFailure;
    }
    normalize_errors(&mut errors);
    drop(run_guard);
    Ok(TraceRunResult {
        round: item.round,
        probe_type: item.probe_type,
        host: item.host.clone(),
        target: target.ip(),
        hops,
        status,
        planned_cycles: configured.cycles,
        completed_cycles,
        enrichment_complete,
        errors,
    })
}

fn extend_errors(errors: &mut Vec<String>, incoming: impl IntoIterator<Item = String>) {
    const LIMIT: usize = MAX_ERROR_BYTES - 256; // reserve space for terminal status diagnostics
    let mut retained: usize = errors.iter().map(String::len).sum();
    for mut message in incoming {
        if retained >= LIMIT {
            break;
        }
        let mut end = message.len().min(LIMIT - retained);
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        retained += message.len();
        errors.push(message);
    }
}

fn normalize_errors(errors: &mut Vec<String>) {
    let mut retained = 0usize;
    errors.retain_mut(|message| {
        if retained >= MAX_ERROR_BYTES {
            return false;
        }
        let mut end = message.len().min(MAX_ERROR_BYTES - retained);
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        retained += message.len();
        !message.is_empty()
    });
}

fn settle_sample(mut sample: path_io::Sample, normal_grace_expired: bool) -> path_io::Sample {
    if normal_grace_expired && sample.status == path_io::SampleStatus::Cancelled {
        sample.status = path_io::SampleStatus::Timeout;
        sample.elapsed_ms = None;
        sample.hop = None;
        sample.next_hop_mtu = None;
        sample.mpls.clear();
    }
    sample
}

fn final_reply_deadline(cadence: &Cadence) -> tokio::time::Instant {
    cadence.deadline + FINAL_REPLY_GRACE
}

#[cfg(test)]
fn summarize_hops(samples_by_hop: Vec<Vec<Sample>>, first_ttl: u8) -> Vec<TraceHop> {
    samples_by_hop
        .into_iter()
        .enumerate()
        .skip(usize::from(first_ttl))
        .filter_map(|(ttl, samples)| {
            let mut accumulator = HopAccumulator::default();
            for sample in samples {
                accumulator.record(sample);
            }
            accumulator.finish(ttl as u8)
        })
        .collect()
}

fn trace_probe(
    probe: &CycleProbe,
    cycle: u16,
    ttl: u8,
    cancellation: &CancellationToken,
) -> Result<path_io::HopResult, ProbeError> {
    let mut parameters = probe.parameters.clone();
    parameters.max_hops = ttl;
    let identity = path_io::ProbeIdentity {
        nonce: probe.nonce,
        ordinal: u32::from(cycle) * (u32::from(probe.configured.max_hops) + 1) + u32::from(ttl),
    };
    match probe.probe_type {
        TraceType::Udp4 | TraceType::Udp6 => path_io::udp_trace_probe_scoped(
            probe.target,
            ttl,
            &parameters,
            probe.configured.udp_base_port,
            identity,
            cancellation,
        ),
        TraceType::Tcp4 | TraceType::Tcp6 => path_io::tcp_trace_probe_scoped(
            probe.target,
            ttl,
            &parameters,
            probe.configured.tcp_port,
            identity,
            cancellation,
        ),
        _ => path_io::trace_probe_scoped(probe.target, ttl, &parameters, identity, cancellation),
    }
}
struct CycleProbe {
    target: SocketAddr,
    probe_type: TraceType,
    configured: TraceSettings,
    parameters: RoundParameters,
    nonce: u32,
}

async fn enrich_hops(
    hops: &mut [TraceHop],
    resolver: SocketAddr,
    timeout: Duration,
    needs_asn: bool,
    cancellation: &CancellationToken,
) -> Vec<String> {
    let mut pending = hops
        .iter()
        .filter_map(|hop| hop.address)
        .collect::<Vec<_>>();
    pending.sort_unstable();
    pending.dedup();
    let mut cache = HashMap::new();
    let mut tasks = JoinSet::new();
    let mut errors = Vec::new();
    while !pending.is_empty() || !tasks.is_empty() {
        while tasks.len() < MAX_DNS_QUERIES && !pending.is_empty() {
            let address = pending.pop().expect("pending DNS address");
            let token = cancellation.clone();
            tasks.spawn(async move {
                let mut query_errors = Vec::new();
                let hostname = match dns::reverse_name(address, resolver, timeout, &token).await {
                    Ok(value) => value,
                    Err(error) => {
                        query_errors.push(format!("PTR {address}: {error}"));
                        None
                    }
                };
                let asn = if needs_asn {
                    match dns::lookup_asn(address, resolver, timeout, &token).await {
                        Ok(value) => value,
                        Err(error) => {
                            query_errors.push(format!("ASN {address}: {error}"));
                            None
                        }
                    }
                } else {
                    None
                };
                (address, hostname, asn, query_errors)
            });
        }
        match tasks.join_next().await {
            Some(Ok((address, hostname, asn, query_errors))) => {
                cache.insert(address, (hostname, asn));
                errors.extend(query_errors);
            }
            Some(Err(error)) => errors.push(format!("DNS enrichment task failed: {error}")),
            None => break,
        }
    }
    for hop in hops {
        if let Some(values) = hop.address.and_then(|address| cache.get(&address)) {
            hop.hostname = values.0.clone();
            hop.asn = values.1.clone();
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_vocabularies_match_mtr_contract() {
        let settings = TraceSettings::default();
        assert_eq!(settings.cycles, 300);
        assert_eq!(settings.interval, Duration::from_secs(1));
        assert_eq!(settings.run_timeout, Duration::from_secs(360));
        assert_eq!(TraceType::ALL.len(), 10);
        assert_eq!(TraceRound::ALL.len(), 8);
        assert!(settings.validate().is_ok());
    }
    #[test]
    fn matrix_selects_hosts_by_address_family() {
        let plan = build_plan(
            &[TraceRound::Standard, TraceRound::Ttl64],
            &[TraceType::Icmp4, TraceType::Icmp6],
            &["v4.example".into()],
            &["v6.example".into()],
        )
        .unwrap();
        assert_eq!(plan.total_items, 4);
        assert_eq!(plan.items().nth(1).unwrap().host, "v6.example");
    }
    #[test]
    fn round_overrides_match_legacy_flags() {
        let defaults = TraceSettings::default();
        assert_eq!(TraceRound::Mtu1400.apply(&defaults).packet_size, 1400);
        assert_eq!(TraceRound::Ttl10.apply(&defaults).max_hops, 10);
        assert_eq!(TraceRound::FirstTtl3.apply(&defaults).first_ttl, 3);
        assert_eq!(
            TraceRound::Timeout5.apply(&defaults).probe_timeout,
            Duration::from_secs(5)
        );
        assert_eq!(TraceRound::TosCs5.traffic_class(), Some(160));
    }

    #[test]
    fn rejects_probe_identity_port_wrap() {
        let settings = TraceSettings {
            cycles: 300,
            max_hops: 64,
            udp_base_port: 60_000,
            ..TraceSettings::default()
        };
        assert!(matches!(
            settings.validate_for_type(TraceType::Udp4),
            Err(ProbeError::InvalidPlan(_))
        ));
    }

    #[test]
    fn preserves_ecmp_responders_and_distinct_mpls_stacks() {
        let first: IpAddr = "192.0.2.1".parse().unwrap();
        let second: IpAddr = "192.0.2.2".parse().unwrap();
        let stack = vec![lantern_packet::MplsLabel {
            label: 16,
            traffic_class: 0,
            bottom_of_stack: true,
            ttl: 63,
        }];
        let samples = vec![
            Sample {
                sequence: 1,
                hop: Some(first),
                elapsed_ms: Some(1.0),
                status: path_io::SampleStatus::TimeExceeded,
                next_hop_mtu: None,
                mpls: stack.clone(),
            },
            Sample {
                sequence: 2,
                hop: Some(second),
                elapsed_ms: Some(2.0),
                status: path_io::SampleStatus::TimeExceeded,
                next_hop_mtu: None,
                mpls: Vec::new(),
            },
            Sample {
                sequence: 3,
                hop: Some(first),
                elapsed_ms: Some(3.0),
                status: path_io::SampleStatus::TimeExceeded,
                next_hop_mtu: None,
                mpls: stack,
            },
        ];
        let hops = summarize_hops(vec![Vec::new(), samples], 1);
        assert_eq!(hops[0].responders.len(), 2);
        assert_eq!(hops[0].responders[0].statistics.received, 2);
        assert_eq!(hops[0].responders[1].statistics.received, 1);
        assert_eq!(hops[0].mpls_stacks.len(), 1);
    }

    #[test]
    fn final_grace_starts_after_next_cadence_tick_and_finalizes_loss() {
        let start = tokio::time::Instant::now();
        let mut cadence = Cadence::new(start, Duration::from_secs(1), 1, 1);
        cadence.dispatched(cadence.deadline, &[false, true], Some(1));
        assert_eq!(
            final_reply_deadline(&cadence),
            cadence.deadline + Duration::from_secs(5)
        );

        let cancelled = Sample {
            sequence: 1,
            hop: None,
            elapsed_ms: None,
            status: path_io::SampleStatus::Cancelled,
            next_hop_mtu: None,
            mpls: Vec::new(),
        };
        let pending = settle_sample(cancelled.clone(), false);
        let pending_stats = Statistics::from_samples(&[pending]);
        assert_eq!(pending_stats.sent, 1);
        assert_eq!(pending_stats.cancelled, 1);
        assert_eq!(pending_stats.loss_percent, 0.0);

        let finalized = settle_sample(cancelled, true);
        assert_eq!(finalized.status, path_io::SampleStatus::Timeout);
        let finalized_stats = Statistics::from_samples(&[finalized]);
        assert_eq!(finalized_stats.sent, 1);
        assert_eq!(finalized_stats.cancelled, 0);
        assert_eq!(finalized_stats.loss_percent, 100.0);
    }

    #[test]
    fn probe_only_cancellation_leaves_run_token_available_for_enrichment() {
        let run = CancellationToken::new();
        let probe = run.child_token();
        probe.cancel();
        assert!(probe.is_cancelled());
        assert!(!run.is_cancelled());
    }

    #[test]
    fn terminal_error_normalization_is_utf8_safe_and_bounded() {
        let mut errors = vec!["é".repeat(MAX_ERROR_BYTES), "terminal".into()];
        normalize_errors(&mut errors);
        assert!(errors.iter().map(String::len).sum::<usize>() <= MAX_ERROR_BYTES);
        assert!(
            errors
                .iter()
                .all(|message| message.is_char_boundary(message.len()))
        );
    }

    fn no_reply_probe(
        _: &CycleProbe,
        _: u16,
        ttl: u8,
        _: &CancellationToken,
    ) -> Result<path_io::HopResult, ProbeError> {
        Ok(path_io::HopResult {
            ttl,
            address: None,
            reached_destination: false,
            samples: vec![Sample {
                sequence: ttl.into(),
                hop: None,
                elapsed_ms: None,
                status: path_io::SampleStatus::Timeout,
                next_hop_mtu: None,
                mpls: Vec::new(),
            }],
        })
    }

    fn awaiting_probe(
        probe: &CycleProbe,
        cycle: u16,
        ttl: u8,
        cancellation: &CancellationToken,
    ) -> Result<path_io::HopResult, ProbeError> {
        while !cancellation.is_cancelled() {
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut hop = no_reply_probe(probe, cycle, ttl, cancellation)?;
        hop.samples[0].status = path_io::SampleStatus::Cancelled;
        Ok(hop)
    }

    fn isolated_trace() -> (TracePlanItem, TraceSettings) {
        (
            TracePlanItem {
                round: TraceRound::Standard,
                probe_type: TraceType::Icmp4,
                host: "127.0.0.1".into(),
            },
            TraceSettings {
                cycles: 1,
                interval: Duration::from_millis(10),
                max_hops: 1,
                // No addresses are returned by the injected boundary, so no DNS
                // enrichment is requested. No socket is opened by these tests.
                dns_resolver: Some("127.0.0.1:9".parse().unwrap()),
                ..TraceSettings::default()
            },
        )
    }

    #[tokio::test]
    async fn final_window_runs_even_when_every_probe_has_finished() {
        let (item, settings) = isolated_trace();
        let start = std::time::Instant::now();
        let result =
            execute_run_with_probe(&item, &settings, &CancellationToken::new(), no_reply_probe)
                .await
                .unwrap();
        assert!(start.elapsed() >= FINAL_REPLY_GRACE + settings.interval);
        assert_eq!(result.completed_cycles, 1);
        assert_eq!(result.hops[0].statistics.sent, 1);
        assert_eq!(result.hops[0].statistics.loss_percent, 100.0);
    }

    #[tokio::test]
    async fn final_window_interrupts_unresolved_probes_as_loss() {
        let (item, settings) = isolated_trace();
        let start = std::time::Instant::now();
        let result =
            execute_run_with_probe(&item, &settings, &CancellationToken::new(), awaiting_probe)
                .await
                .unwrap();
        assert!(start.elapsed() < settings.probe_timeout);
        assert_eq!(result.completed_cycles, 1);
        assert_eq!(result.hops[0].statistics.sent, 1);
        assert_eq!(result.hops[0].statistics.cancelled, 0);
        assert_eq!(result.hops[0].statistics.loss_percent, 100.0);
        assert!(!result.errors.iter().any(|error| error.contains("shutdown")));
    }
}
