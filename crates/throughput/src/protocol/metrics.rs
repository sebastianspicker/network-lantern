use super::*;
use std::collections::HashSet;

pub(super) fn build_result(
    spec: TestSpec,
    attempts: u8,
    elapsed: Duration,
    outcomes: Vec<StreamOutcome>,
    server_result: Value,
) -> Result<TestResult> {
    let streams = server_result
        .get("streams")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("Missing streams array"))?;
    if streams.len() != outcomes.len() {
        return Err(bad("Peer stream count does not match established streams"));
    }
    let mut ids = HashSet::new();
    let mut remote_bytes = 0u64;
    let mut remote_rate = 0.0;
    let mut retransmits = None;
    for stream in streams {
        let id = integer(stream, "id")?;
        if !ids.insert(id) || !outcomes.iter().any(|o| o.id as u64 == id) {
            return Err(bad("Unknown or duplicate stream ID"));
        }
        let bytes = integer(stream, "bytes")?;
        remote_bytes = remote_bytes
            .checked_add(bytes)
            .ok_or_else(|| bad("Peer byte count overflow"))?;
        let start = number(stream, "start_time")?;
        let end = number(stream, "end_time")?;
        if end <= start {
            return Err(bad("Non-positive peer measurement duration"));
        }
        remote_rate += bytes as f64 * 8.0 / (end - start) / 1_000_000.0;
        if let Some(value) = stream
            .get("retransmits")
            .and_then(Value::as_i64)
            .filter(|n| *n >= 0)
        {
            retransmits = Some(
                retransmits
                    .unwrap_or(0u64)
                    .checked_add(value as u64)
                    .ok_or_else(|| bad("Retransmit counter overflow"))?,
            );
        }
        if spec.protocol == Protocol::Udp {
            let packets = integer(stream, "packets")?;
            let errors = integer(stream, "errors")?;
            if errors > packets {
                return Err(bad("Peer loss count exceeds packets"));
            }
            number(stream, "jitter")?;
        }
    }
    let sum_bytes = |sender| {
        outcomes
            .iter()
            .filter(|o| o.sender == sender)
            .try_fold(0u64, |sum, o| {
                sum.checked_add(o.bytes)
                    .ok_or_else(|| bad("Local byte count overflow"))
            })
    };
    let sum_rate = |sender| {
        outcomes
            .iter()
            .filter(|o| o.sender == sender)
            .try_fold(0.0, |sum, o| {
                if !o.duration_seconds.is_finite() || o.duration_seconds <= 0.0 {
                    return Err(bad("Invalid local measurement duration"));
                }
                Ok(sum + o.bytes as f64 * 8.0 / o.duration_seconds / 1_000_000.0)
            })
    };
    let (bytes_sent, bytes_received, tx_mbps, rx_mbps) = match spec.direction {
        Direction::Tx => (
            sum_bytes(true)?,
            remote_bytes,
            Some(sum_rate(true)?),
            Some(remote_rate),
        ),
        Direction::Rx => (
            remote_bytes,
            sum_bytes(false)?,
            Some(remote_rate),
            Some(sum_rate(false)?),
        ),
        Direction::Bidirectional => (
            sum_bytes(true)?,
            sum_bytes(false)?,
            Some(sum_rate(true)?),
            Some(sum_rate(false)?),
        ),
    };
    let (loss_pct, jitter_ms) = udp_metrics(&spec, &outcomes, streams)?;
    let mut unavailable_metrics = Vec::new();
    if spec.protocol == Protocol::Udp && spec.direction == Direction::Tx && spec.omit_secs > 0 {
        unavailable_metrics.push("Post-omission receiver loss: iperf3 control results omit the receiver's warm-up loss baseline".into());
    }
    if spec.protocol == Protocol::Tcp && retransmits.is_none() {
        unavailable_metrics.push("TCP sender retransmit counter not supplied by the peer".into());
    }
    let encoded = serde_json::to_string(&server_result)?;
    let (server_result, server_result_truncated) = bounded_diagnostic(&encoded, 16 * 1024);
    let loss_scope = (spec.protocol == Protocol::Udp).then(|| {
        if spec.direction == Direction::Tx {
            "whole_run".into()
        } else {
            "post_omit".into()
        }
    });
    Ok(TestResult {
        spec,
        attempts,
        elapsed_ms: elapsed.as_millis().min(u64::MAX as u128) as u64,
        bytes_sent,
        bytes_received,
        tx_mbps,
        rx_mbps,
        retransmits,
        loss_pct,
        loss_scope,
        jitter_ms,
        server_result,
        server_result_truncated,
        unavailable_metrics,
    })
}
fn bad(message: &str) -> ThroughputError {
    ThroughputError::MalformedResult(message.into())
}
fn integer(value: &Value, key: &str) -> Result<u64> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| bad(&format!("Invalid {key} counter")))
}
fn number(value: &Value, key: &str) -> Result<f64> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0)
        .ok_or_else(|| bad(&format!("Invalid {key} metric")))
}
pub(super) fn bounded_diagnostic(text: &str, maximum: usize) -> (String, bool) {
    if text.len() <= maximum {
        return (text.into(), false);
    }
    let mut end = maximum;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].into(), true)
}
fn udp_metrics(
    spec: &TestSpec,
    local: &[StreamOutcome],
    remote: &[Value],
) -> Result<(Option<f64>, Option<f64>)> {
    if spec.protocol != Protocol::Udp {
        return Ok((None, None));
    }
    if spec.direction != Direction::Tx {
        let packets = local
            .iter()
            .filter(|o| !o.sender)
            .map(|o| o.packets)
            .sum::<u64>();
        let errors = local
            .iter()
            .filter(|o| !o.sender)
            .map(|o| o.errors)
            .sum::<u64>();
        let jitter = local
            .iter()
            .filter(|o| !o.sender)
            .map(|o| o.jitter_seconds)
            .reduce(f64::max);
        return Ok((
            (packets > 0).then(|| errors as f64 * 100.0 / packets as f64),
            jitter.map(|j| j * 1000.0),
        ));
    }
    let packets = remote.iter().try_fold(0u64, |s, o| {
        s.checked_add(integer(o, "packets")?)
            .ok_or_else(|| bad("Packet count overflow"))
    })?;
    let errors = remote.iter().try_fold(0u64, |s, o| {
        s.checked_add(integer(o, "errors")?)
            .ok_or_else(|| bad("Loss count overflow"))
    })?;
    let jitter = remote
        .iter()
        .map(|o| number(o, "jitter"))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .reduce(f64::max);
    Ok((
        (packets > 0).then(|| errors as f64 * 100.0 / packets as f64),
        jitter.map(|j| j * 1000.0),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spec() -> TestSpec {
        crate::plan(
            &crate::SuiteConfig {
                target: "127.0.0.1".into(),
                single_test: true,
                ..Default::default()
            },
            Default::default(),
        )
        .unwrap()
        .tests()
        .next()
        .unwrap()
    }
    fn local() -> Vec<StreamOutcome> {
        vec![StreamOutcome {
            id: 1,
            sender: true,
            bytes: 1_000_000,
            packets: 0,
            errors: 0,
            jitter_seconds: 0.0,
            duration_seconds: 2.0,
            wire_packets: 0,
            wire_errors: 0,
        }]
    }
    #[test]
    fn rates_use_measured_local_and_peer_time() {
        let result=build_result(spec(),1,Duration::from_secs(12),local(),json!({"streams":[{"id":1,"bytes":900000,"start_time":0,"end_time":3,"retransmits":-1}]})).unwrap();
        assert_eq!(result.tx_mbps, Some(4.0));
        assert_eq!(result.rx_mbps, Some(2.4));
        assert_eq!(result.retransmits, None);
    }
    #[test]
    fn malformed_peer_results_fail() {
        for result in [
            json!({}),
            json!({"streams":[]}),
            json!({"streams":[{"id":9,"bytes":1,"start_time":0,"end_time":1}]}),
            json!({"streams":[{"id":1,"bytes":-1,"start_time":0,"end_time":1}]}),
            json!({"streams":[{"id":1,"bytes":1,"start_time":1,"end_time":0}]}),
        ] {
            assert!(build_result(spec(), 1, Duration::from_secs(1), local(), result).is_err());
        }
    }
}
