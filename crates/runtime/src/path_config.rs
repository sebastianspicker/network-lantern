use lantern_contracts::{Error, Result};
use lantern_packet::AddressFamily;
use lantern_path_basic::{BasicPlan, BasicRound, BasicSettings};
use lantern_path_trace::{TracePlan, TraceRound, TraceSettings, TraceType};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathConfig {
    pub hosts_ipv4: Vec<String>,
    pub hosts_ipv6: Vec<String>,
    pub protocols: Vec<String>,
    pub rounds: Vec<String>,
    pub types: Vec<String>,
    pub skip_pathping: bool,
    pub ping_count: u16,
    pub max_hops: u8,
    pub timeout_ms: u64,
    pub pathping_probes: u16,
    pub pathping_timeout_ms: u64,
    pub tcp_port: u16,
    pub cycles: u16,
    pub interval_ms: u64,
    pub run_timeout_ms: u64,
}
impl Default for PathConfig {
    fn default() -> Self {
        Self {
            hosts_ipv4: lantern_path_basic::DEFAULT_IPV4_HOSTS
                .iter()
                .map(|s| (*s).into())
                .collect(),
            hosts_ipv6: lantern_path_basic::DEFAULT_IPV6_HOSTS
                .iter()
                .map(|s| (*s).into())
                .collect(),
            protocols: vec!["IPv4".into(), "IPv6".into()],
            rounds: Vec::new(),
            types: TraceType::DEFAULTS
                .iter()
                .map(|t| t.name().into())
                .collect(),
            skip_pathping: false,
            ping_count: 5,
            max_hops: 30,
            timeout_ms: 5000,
            pathping_probes: 50,
            pathping_timeout_ms: 3000,
            tcp_port: 443,
            cycles: 300,
            interval_ms: 1000,
            run_timeout_ms: 360000,
        }
    }
}
impl PathConfig {
    pub fn validate(&self) -> Result<()> {
        if self.hosts_ipv4.len() + self.hosts_ipv6.len() > 1024
            || self.rounds.len() > 256
            || self.types.len() > 256
            || self.protocols.len() > 256
        {
            return Err(Error::validation(
                "Path matrix input exceeds bounded list limits",
            ));
        }
        if self.max_hops == 0
            || self.tcp_port == 0
            || self.ping_count == 0
            || self.ping_count > 1000
            || self.pathping_probes > 1000
            || self.cycles == 0
            || self.cycles > 10000
        {
            return Err(Error::validation(
                "Path counts, hops or port outside supported limits",
            ));
        }
        if self.timeout_ms == 0
            || self.timeout_ms > 300000
            || self.pathping_timeout_ms == 0
            || self.pathping_timeout_ms > 300000
            || self.interval_ms == 0
            || self.interval_ms > 60000
            || self.run_timeout_ms == 0
            || self.run_timeout_ms > 86_400_000
        {
            return Err(Error::validation("Path timing outside supported limits"));
        }
        Ok(())
    }
    pub fn basic(&self) -> Result<(BasicPlan, BasicSettings)> {
        self.validate()?;
        let rounds = if self.rounds.is_empty() {
            vec![BasicRound::Standard]
        } else {
            self.rounds
                .iter()
                .map(|s| {
                    BasicRound::ALL
                        .iter()
                        .find(|r| r.name().eq_ignore_ascii_case(s))
                        .copied()
                        .ok_or_else(|| Error::validation(format!("Unknown basic round {s}")))
                })
                .collect::<Result<_>>()?
        };
        let families = self
            .protocols
            .iter()
            .map(|p| match p.to_ascii_lowercase().as_str() {
                "ipv4" => Ok(AddressFamily::Ipv4),
                "ipv6" => Ok(AddressFamily::Ipv6),
                _ => Err(Error::validation("Unknown IP family")),
            })
            .collect::<Result<Vec<_>>>()?;
        let plan =
            lantern_path_basic::build_plan(&rounds, &families, &self.hosts_ipv4, &self.hosts_ipv6)
                .map_err(|e| Error::validation(e.to_string()))?;
        let settings = BasicSettings {
            ping_count: self.ping_count,
            trace_max_hops: self.max_hops,
            trace_timeout: Duration::from_millis(self.timeout_ms),
            pathping_probes: self.pathping_probes,
            pathping_timeout: Duration::from_millis(self.pathping_timeout_ms),
            tcp_port: self.tcp_port,
            skip_pathping: self.skip_pathping,
        };
        settings
            .validate()
            .map_err(|e| Error::validation(e.to_string()))?;
        Ok((plan, settings))
    }
    pub fn trace(&self) -> Result<(TracePlan, TraceSettings)> {
        self.validate()?;
        let rounds = if self.rounds.is_empty() {
            vec![TraceRound::Standard]
        } else {
            self.rounds
                .iter()
                .map(|s| {
                    TraceRound::ALL
                        .iter()
                        .find(|r| r.name().eq_ignore_ascii_case(s))
                        .copied()
                        .ok_or_else(|| Error::validation(format!("Unknown trace round {s}")))
                })
                .collect::<Result<_>>()?
        };
        let types = self
            .types
            .iter()
            .map(|s| {
                TraceType::ALL
                    .iter()
                    .find(|t| t.name().eq_ignore_ascii_case(s))
                    .copied()
                    .ok_or_else(|| Error::validation(format!("Unknown trace type {s}")))
            })
            .collect::<Result<Vec<_>>>()?;
        let plan =
            lantern_path_trace::build_plan(&rounds, &types, &self.hosts_ipv4, &self.hosts_ipv6)
                .map_err(|e| Error::validation(e.to_string()))?;
        let settings = TraceSettings {
            cycles: self.cycles,
            interval: Duration::from_millis(self.interval_ms),
            run_timeout: Duration::from_millis(self.run_timeout_ms),
            probe_timeout: Duration::from_millis(self.timeout_ms),
            max_hops: self.max_hops,
            tcp_port: self.tcp_port,
            ..TraceSettings::default()
        };
        for round in rounds {
            for probe_type in &types {
                round
                    .apply(&settings)
                    .validate_for_type(*probe_type)
                    .map_err(|e| Error::validation(e.to_string()))?;
            }
        }
        Ok((plan, settings))
    }
}
pub fn translate(layer: &Value) -> Result<Value> {
    let object = layer
        .as_object()
        .ok_or_else(|| Error::validation("Path settings must be an object"))?;
    let mut output = serde_json::Map::new();
    for (key, value) in object {
        let name = match key.to_ascii_lowercase().as_str() {
            "hostsipv4" => "hosts_ipv4",
            "hostsipv6" => "hosts_ipv6",
            "skippathping" => "skip_pathping",
            _ => key,
        };
        if key == "target" {
            let host = value
                .as_str()
                .ok_or_else(|| Error::validation("target must be a string"))?;
            let ipv6 = host.parse::<std::net::Ipv6Addr>().is_ok();
            output.insert(
                "hosts_ipv4".into(),
                if ipv6 { json!([]) } else { json!([host]) },
            );
            output.insert(
                "hosts_ipv6".into(),
                if ipv6 { json!([host]) } else { json!([]) },
            );
            output.insert(
                "protocols".into(),
                json!([if ipv6 { "IPv6" } else { "IPv4" }]),
            );
            continue;
        }
        // Legacy workflow aliases omit empty host selections to retain fallbacks.
        // Native host lists are resolved control input: [] excludes that family.
        let legacy_empty_hosts =
            matches!(key.to_ascii_lowercase().as_str(), "hostsipv4" | "hostsipv6");
        if (legacy_empty_hosts || matches!(name, "rounds" | "protocols"))
            && value.as_array().is_some_and(Vec::is_empty)
        {
            continue;
        }
        output.insert(name.into(), value.clone());
    }
    Ok(Value::Object(output))
}
pub fn resolve(layers: &[Value], strict: bool, trace: bool) -> Result<(PathConfig, Vec<String>)> {
    let layers = layers.iter().map(translate).collect::<Result<Vec<_>>>()?;
    let (value, warnings) = crate::config::merge_layers(
        serde_json::to_value(if trace {
            PathConfig {
                timeout_ms: 10000,
                ..PathConfig::default()
            }
        } else {
            PathConfig::default()
        })
        .unwrap(),
        &layers,
        strict,
    )?;
    let config = serde_json::from_value(value).map_err(|e| Error::validation(e.to_string()))?;
    Ok((config, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_empty_host_lists_override_defaults_and_earlier_layers() {
        let (config, _) = resolve(
            &[
                json!({"hosts_ipv4":["192.0.2.1"],"hosts_ipv6":["2001:db8::1"]}),
                json!({"hosts_ipv6":[]}),
            ],
            true,
            true,
        )
        .unwrap();
        assert_eq!(config.hosts_ipv4, ["192.0.2.1"]);
        assert!(config.hosts_ipv6.is_empty());
        assert_eq!(config.trace().unwrap().0.total_items, 2);

        let (empty, _) = resolve(&[json!({"hosts_ipv4":[],"hosts_ipv6":[]})], true, true).unwrap();
        assert!(empty.trace().is_err());
        assert!(empty.basic().is_err());
    }

    #[test]
    fn legacy_empty_aliases_and_selection_lists_keep_fallbacks() {
        let defaults = PathConfig::default();
        let (config, _) = resolve(
            &[json!({"HostsIPv4":[],"HostsIPv6":[],"protocols":[],"rounds":[]})],
            true,
            false,
        )
        .unwrap();
        assert_eq!(config.hosts_ipv4, defaults.hosts_ipv4);
        assert_eq!(config.hosts_ipv6, defaults.hosts_ipv6);
        assert_eq!(config.protocols, defaults.protocols);
        assert!(config.rounds.is_empty());
        assert!(config.basic().is_ok());
    }
}
