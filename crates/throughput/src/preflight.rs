//! Throughput connectivity checks, separately recorded from throughput measurements.
use crate::{IpVersion, SuiteConfig};
use lantern_contracts::{Error, ErrorCategory, Result};
use lantern_packet::AddressFamily;
use lantern_path_io::{RoundParameters, SampleStatus};
use serde_json::{Value, json};
use std::{net::IpAddr, time::Duration};
use tokio_util::sync::CancellationToken;

pub async fn preflight_check(
    config: &SuiteConfig,
    cancel: &CancellationToken,
) -> Result<(IpAddr, Value)> {
    let child = cancel.child_token();
    let _guard = child.clone().drop_guard();
    let families = match config.ip_version {
        IpVersion::Ipv4 => vec![AddressFamily::Ipv4],
        IpVersion::Ipv6 => vec![AddressFamily::Ipv6],
        IpVersion::Auto => vec![AddressFamily::Ipv4, AddressFamily::Ipv6],
    };
    let timeout = Duration::from_millis(config.connect_timeout_ms);
    let mut selected = None;
    let mut attempts = Vec::new();
    let mut prerequisite_error = None;
    for family in families {
        let address = tokio::select! {_ = child.cancelled()=>return Err(cancelled()), value=tokio::time::timeout(timeout,lantern_path_io::resolve_target(&config.target,family,&child))=>match value {
            Ok(Ok(address))=>address,
            error=>{attempts.push(format!("{family:?}: {error:?}"));continue;}
        }};
        if !config.skip_reachability_check {
            let token = child.clone();
            let samples = tokio::task::spawn_blocking(move || {
                lantern_path_io::ping(
                    address,
                    1,
                    128,
                    &parameters(32, false, Duration::from_secs(4)),
                    &token,
                )
            })
            .await
            .map_err(|e| Error::new(ErrorCategory::Internal, e.to_string()))?;
            match samples {
                Ok(samples) if samples.iter().any(|s| s.status == SampleStatus::Reply) => (),
                Err(lantern_path_io::ProbeError::Permission { .. }) => {
                    prerequisite_error = Some(Error::new(
                        ErrorCategory::Permission,
                        "Native ICMP requires platform authorization; register the helper or explicitly skip reachability checks",
                    ));
                    continue;
                }
                Err(lantern_path_io::ProbeError::Unsupported { message, .. }) => {
                    prerequisite_error = Some(Error::new(ErrorCategory::Prerequisite, message));
                    continue;
                }
                result => {
                    attempts.push(format!("{family:?} ICMP: {result:?}"));
                    continue;
                }
            }
        }
        selected = Some(address);
        break;
    }
    if child.is_cancelled() {
        return Err(cancelled());
    }
    let address =
        selected.ok_or_else(|| {
            prerequisite_error.unwrap_or_else(|| Error::new(
            ErrorCategory::Connectivity,
            format!(
                "Reachability failed; skip_reachability_check permits TCP-only networks. {}",
                attempts.join("; ")
            ),
        ))
        })?;
    if !lantern_path_io::test_tcp(address, config.port, timeout, &child)
        .await
        .map_err(|e| Error::new(ErrorCategory::Connectivity, e.to_string()))?
    {
        return Err(Error::new(
            ErrorCategory::Connectivity,
            "iperf3 TCP port is not reachable",
        ));
    }
    let trace_token = child.child_token();
    let _trace_guard = trace_token.clone().drop_guard();
    let token = trace_token.clone();
    let trace = tokio::task::spawn_blocking(move || {
        lantern_path_io::trace(
            address,
            1,
            &parameters(32, false, Duration::from_secs(1)),
            &token,
        )
    });
    let trace = tokio::select! {_ = child.cancelled()=>return Err(cancelled()), value=tokio::time::timeout(Duration::from_secs(10),trace)=>match value {
        Ok(Ok(Ok(hops)))=>json!({"hops":hops}),
        result=>{trace_token.cancel();json!({"error":format!("{result:?}")})}
    }};
    let mut mtu = Vec::new();
    if !config.disable_mtu_probe {
        for size in &config.mtu_sizes {
            if child.is_cancelled() {
                return Err(cancelled());
            }
            let size = *size;
            let token = child.clone();
            let result = tokio::task::spawn_blocking(move || {
                lantern_path_io::ping(
                    address,
                    4,
                    128,
                    &parameters(usize::from(size), address.is_ipv4(), Duration::from_secs(4)),
                    &token,
                )
            })
            .await
            .map_err(|e| Error::new(ErrorCategory::Internal, e.to_string()))?;
            match result {
                Ok(samples)=>mtu.push(json!({"payload_size":size,"succeeded":samples.iter().any(|s|s.status==SampleStatus::Reply),"samples":samples})),
                Err(error)=>mtu.push(json!({"payload_size":size,"succeeded":false,"error":error.to_string()})),
            }
        }
    }
    Ok((
        address,
        json!({"target":config.target,"address":address,"reachability_skipped":config.skip_reachability_check,"tcp_port":config.port,"tcp_reachable":true,"trace":trace,"mtu_probe_disabled":config.disable_mtu_probe,"mtu":mtu}),
    ))
}
fn parameters(payload_size: usize, dont_fragment: bool, timeout: Duration) -> RoundParameters {
    RoundParameters {
        payload_size,
        dont_fragment,
        timeout,
        max_hops: 5,
        traffic_class: None,
    }
}
fn cancelled() -> Error {
    Error::new(ErrorCategory::Cancelled, "Connectivity checks cancelled")
}
