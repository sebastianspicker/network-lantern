//! Incremental hop statistics; memory does not grow with the cycle count.
use crate::{TraceHop, TraceResponder};
use lantern_path_io::{Sample, Statistics};
use std::{collections::BTreeMap, net::IpAddr};

#[derive(Default)]
struct Moments {
    sent: u32,
    received: u32,
    cancelled: u32,
    mean: f64,
    m2: f64,
    last: Option<f64>,
    best: Option<f64>,
    worst: Option<f64>,
}
impl Moments {
    fn record(&mut self, sample: &Sample) {
        self.sent += 1;
        if sample.status == lantern_path_io::SampleStatus::Cancelled {
            self.cancelled += 1;
        }
        if sample.status != lantern_path_io::SampleStatus::Cancelled
            && let Some(value) = sample.elapsed_ms
        {
            self.received += 1;
            let delta = value - self.mean;
            self.mean += delta / f64::from(self.received);
            self.m2 += delta * (value - self.mean);
            self.last = Some(value);
            self.best = Some(self.best.map_or(value, |old| old.min(value)));
            self.worst = Some(self.worst.map_or(value, |old| old.max(value)));
        }
    }
    fn finish(self) -> Statistics {
        let finalized = self.sent - self.cancelled;
        Statistics {
            sent: self.sent,
            received: self.received,
            cancelled: self.cancelled,
            loss_percent: if finalized == 0 {
                0.0
            } else {
                100.0 * f64::from(finalized - self.received) / f64::from(finalized)
            },
            last_ms: self.last,
            average_ms: (self.received > 0).then_some(self.mean),
            best_ms: self.best,
            worst_ms: self.worst,
            stddev_ms: (self.received > 0)
                .then(|| (self.m2.max(0.0) / f64::from(self.received)).sqrt()),
        }
    }
}
#[derive(Default)]
pub(super) struct HopAccumulator {
    totals: Moments,
    responders: BTreeMap<IpAddr, Moments>,
    stacks: Vec<Vec<lantern_packet::MplsLabel>>,
    pub limited: bool,
}
impl HopAccumulator {
    pub fn record(&mut self, sample: Sample) {
        self.totals.record(&sample);
        if let Some(address) = sample.hop {
            if self.responders.len() < 64 || self.responders.contains_key(&address) {
                self.responders.entry(address).or_default().record(&sample);
            } else {
                self.limited = true;
            }
        }
        if !sample.mpls.is_empty() && !self.stacks.contains(&sample.mpls) {
            if self.stacks.len() < 64 {
                self.stacks.push(sample.mpls);
            } else {
                self.limited = true;
            }
        }
    }
    pub fn finish(self, ttl: u8) -> Option<TraceHop> {
        if self.totals.sent == 0 {
            return None;
        }
        Some(TraceHop {
            ttl,
            address: self.responders.keys().next().copied(),
            hostname: None,
            asn: None,
            mpls: self.stacks.iter().flatten().cloned().collect(),
            mpls_stacks: self.stacks,
            responders: self
                .responders
                .into_iter()
                .map(|(address, values)| TraceResponder {
                    address,
                    statistics: values.finish(),
                })
                .collect(),
            statistics: self.totals.finish(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lantern_path_io::SampleStatus;
    #[test]
    fn streaming_statistics_match_recorded_samples_and_bound_retention() {
        let samples: Vec<_> = (0..10000u16)
            .map(|sequence| Sample {
                sequence,
                hop: Some(IpAddr::V4(std::net::Ipv4Addr::new(
                    192,
                    0,
                    (sequence / 255) as u8,
                    (sequence % 255) as u8,
                ))),
                elapsed_ms: (sequence % 5 != 0).then_some(f64::from(sequence % 97) / 3.0),
                status: if sequence % 5 == 0 {
                    SampleStatus::Timeout
                } else {
                    SampleStatus::TimeExceeded
                },
                next_hop_mtu: None,
                mpls: Vec::new(),
            })
            .collect();
        let expected = Statistics::from_samples(&samples);
        let mut accumulator = HopAccumulator::default();
        for sample in samples {
            accumulator.record(sample);
        }
        assert!(accumulator.limited);
        assert_eq!(accumulator.responders.len(), 64);
        let actual = accumulator.finish(1).unwrap().statistics;
        assert_eq!(actual.sent, expected.sent);
        assert_eq!(actual.received, expected.received);
        assert_eq!(actual.last_ms, expected.last_ms);
        assert!((actual.average_ms.unwrap() - expected.average_ms.unwrap()).abs() < 1e-10);
        assert!((actual.stddev_ms.unwrap() - expected.stddev_ms.unwrap()).abs() < 1e-10);
    }
}
