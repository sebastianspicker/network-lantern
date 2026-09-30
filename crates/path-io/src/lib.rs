//! Shared native socket boundary for the independent path capabilities.

mod native;

use lantern_packet::AddressFamily;
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

pub use native::{
    measure_hops, measure_hops_scoped, ping, ping_scoped, ping_scoped_with_ttl, tcp_trace,
    tcp_trace_probe_scoped, tcp_trace_scoped, trace, trace_probe_scoped, trace_scoped, udp_trace,
    udp_trace_probe_scoped, udp_trace_scoped,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeIdentity {
    pub nonce: u32,
    pub ordinal: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundParameters {
    pub payload_size: usize,
    pub dont_fragment: bool,
    pub max_hops: u8,
    pub timeout: Duration,
    pub traffic_class: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub sequence: u16,
    pub hop: Option<IpAddr>,
    pub elapsed_ms: Option<f64>,
    pub status: SampleStatus,
    pub next_hop_mtu: Option<u32>,
    pub mpls: Vec<lantern_packet::MplsLabel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleStatus {
    Reply,
    TimeExceeded,
    Unreachable,
    Timeout,
    /// The probe was sent, but observation stopped before a reply or its
    /// normal timeout. It is retained as an attempt without being counted as
    /// packet loss.
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HopResult {
    pub ttl: u8,
    pub address: Option<IpAddr>,
    pub reached_destination: bool,
    pub samples: Vec<Sample>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Statistics {
    pub sent: u32,
    pub received: u32,
    #[serde(default)]
    pub cancelled: u32,
    pub loss_percent: f64,
    pub last_ms: Option<f64>,
    pub average_ms: Option<f64>,
    pub best_ms: Option<f64>,
    pub worst_ms: Option<f64>,
    pub stddev_ms: Option<f64>,
}

impl Statistics {
    pub fn from_samples(samples: &[Sample]) -> Self {
        let values: Vec<f64> = samples
            .iter()
            .filter(|sample| sample.status != SampleStatus::Cancelled)
            .filter_map(|sample| sample.elapsed_ms)
            .collect();
        let sent = samples.len() as u32;
        let received = values.len() as u32;
        let cancelled = samples
            .iter()
            .filter(|sample| sample.status == SampleStatus::Cancelled)
            .count() as u32;
        let finalized = sent - cancelled;
        let average =
            (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64);
        let stddev = average.map(|mean| {
            (values
                .iter()
                .map(|value| (value - mean).powi(2))
                .sum::<f64>()
                / values.len() as f64)
                .sqrt()
        });
        Self {
            sent,
            received,
            cancelled,
            loss_percent: if finalized == 0 {
                0.0
            } else {
                f64::from(finalized - received) * 100.0 / f64::from(finalized)
            },
            last_ms: values.last().copied(),
            average_ms: average,
            best_ms: values.iter().copied().reduce(f64::min),
            worst_ms: values.iter().copied().reduce(f64::max),
            stddev_ms: stddev,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HopStatistics {
    pub ttl: u8,
    pub address: Option<IpAddr>,
    pub statistics: Statistics,
}

#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("invalid host: {0}")]
    InvalidHost(String),
    #[error("invalid diagnostic plan: {0}")]
    InvalidPlan(String),
    #[error("DNS resolution failed for {host}: {message}")]
    Dns { host: String, message: String },
    #[error("no {family:?} address was resolved for {host}")]
    AddressFamilyUnavailable { host: String, family: AddressFamily },
    #[error("native {operation} requires permission: {message}")]
    Permission {
        operation: &'static str,
        message: String,
    },
    #[error("native {operation} is unsupported on this platform: {message}")]
    Unsupported {
        operation: &'static str,
        message: String,
    },
    #[error("native {operation} failed: {message}")]
    Io {
        operation: &'static str,
        message: String,
    },
    #[error("operation cancelled")]
    Cancelled,
    #[error("operation timed out after {0:?}")]
    TimedOut(Duration),
}

pub fn validate_host(host: &str) -> Result<(), ProbeError> {
    if host.is_empty()
        || host.starts_with('-')
        || host.chars().any(char::is_whitespace)
        || host.chars().any(char::is_control)
        || host
            .chars()
            .any(|c| matches!(c, '/' | '|' | ';' | '&' | '`' | '$'))
    {
        Err(ProbeError::InvalidHost(host.to_owned()))
    } else {
        Ok(())
    }
}

pub async fn resolve_target(
    host: &str,
    family: AddressFamily,
    cancellation: &CancellationToken,
) -> Result<IpAddr, ProbeError> {
    resolve_target_scoped(host, family, cancellation)
        .await
        .map(|address| address.ip())
}

pub async fn resolve_target_scoped(
    host: &str,
    family: AddressFamily,
    cancellation: &CancellationToken,
) -> Result<SocketAddr, ProbeError> {
    validate_host(host)?;
    let mut addresses = tokio::select! { _ = cancellation.cancelled() => return Err(ProbeError::Cancelled), result = tokio::net::lookup_host((host, 0)) => result.map_err(|error| ProbeError::Dns { host: host.into(), message: error.to_string() })? };
    addresses
        .find(|address| {
            matches!(
                (family, address.ip()),
                (AddressFamily::Ipv4, IpAddr::V4(_)) | (AddressFamily::Ipv6, IpAddr::V6(_))
            )
        })
        .ok_or_else(|| ProbeError::AddressFamilyUnavailable {
            host: host.into(),
            family,
        })
}

pub async fn test_tcp(
    target: IpAddr,
    port: u16,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<bool, ProbeError> {
    test_tcp_scoped(SocketAddr::new(target, port), timeout, cancellation).await
}

pub async fn test_tcp_scoped(
    target: SocketAddr,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<bool, ProbeError> {
    if target.port() == 0 {
        return Err(ProbeError::InvalidPlan("TCP port must be positive".into()));
    }
    let outcome = tokio::select! { _ = cancellation.cancelled() => return Err(ProbeError::Cancelled), result = tokio::time::timeout(timeout, tokio::net::TcpStream::connect(target)) => result };
    match outcome {
        Ok(Ok(_)) => Ok(true),
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => Ok(false),
        Ok(Err(error)) => Err(ProbeError::Io {
            operation: "TCP connect",
            message: error.to_string(),
        }),
        Err(_) => Ok(false),
    }
}
