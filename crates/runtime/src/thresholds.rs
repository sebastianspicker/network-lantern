use lantern_contracts::{Error, Result};
use lantern_throughput::TestResult;
use serde::{Deserialize, Serialize};
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Thresholds {
    pub min_throughput_mbps: Option<f64>,
    pub max_loss_pct: Option<f64>,
    pub max_jitter_ms: Option<f64>,
}
impl Thresholds {
    pub fn validate(&self) -> Result<()> {
        for (name, value, max) in [
            ("min_throughput_mbps", self.min_throughput_mbps, 1_000_000.0),
            ("max_loss_pct", self.max_loss_pct, 100.0),
            ("max_jitter_ms", self.max_jitter_ms, 1_000_000.0),
        ] {
            if value.is_some_and(|v| !v.is_finite() || v < 0.0 || v > max) {
                return Err(Error::validation(format!(
                    "{name} must be between 0 and {max}"
                )));
            }
        }
        Ok(())
    }
    pub fn breaches(&self, result: &TestResult) -> Vec<String> {
        let mut reasons = Vec::new();
        if let Some(min) = self.min_throughput_mbps {
            for (direction, rate) in [("TX", result.tx_mbps), ("RX", result.rx_mbps)] {
                if let Some(rate) = rate.filter(|r| *r < min) {
                    reasons.push(format!("{direction} throughput {rate} < {min} Mbps"));
                }
            }
        }
        if let (Some(max), Some(actual)) = (self.max_loss_pct, result.loss_pct)
            && actual > max
        {
            reasons.push(format!("Loss {actual}% > {max}%"));
        }
        if let (Some(max), Some(actual)) = (self.max_jitter_ms, result.jitter_ms)
            && actual > max
        {
            reasons.push(format!("Jitter {actual} ms > {max} ms"));
        }
        reasons
    }
}
