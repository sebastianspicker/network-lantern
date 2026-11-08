use crate::backup::QosPolicySpec;
use crate::{ComponentStatus, TuningComponent, TuningConfig, TuningResult};

pub(crate) fn evaluate(
    config: &TuningConfig,
    managed_policies: &[QosPolicySpec],
    local_qos_enabled: Option<bool>,
) -> TuningResult {
    let mut result = TuningResult::empty(config);
    result.managed_policies = managed_policies
        .iter()
        .map(|policy| policy.name().into())
        .collect();
    result.missing_ports = config
        .udp_ports
        .iter()
        .copied()
        .filter(|port| !has_port_policy(managed_policies, *port, config.dscp))
        .collect();

    let local_status = match local_qos_enabled {
        Some(true) => ComponentStatus::Ok,
        Some(false) => ComponentStatus::Warn,
        None => ComponentStatus::Unknown,
    };
    let port_status = if result.missing_ports.is_empty() {
        ComponentStatus::Ok
    } else {
        ComponentStatus::Warn
    };
    result
        .components
        .insert(TuningComponent::LocalQos, local_status);
    result
        .components
        .insert(TuningComponent::QosPortPolicies, port_status);
    result
        .components
        .insert(TuningComponent::QosPolicies, port_status);

    if !result.missing_ports.is_empty() {
        result.warnings.push(format!(
            "Missing managed QoS port policies (or DSCP differs from {}) for: {}",
            config.dscp,
            result
                .missing_ports
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    result.success = local_status == ComponentStatus::Ok && port_status == ComponentStatus::Ok;
    result
}

fn has_port_policy(policies: &[QosPolicySpec], port: u16, dscp: u8) -> bool {
    let current = format!("NETWORK_LANTERN_QOS_PORT_{port}");
    let legacy = format!("NDS_QOS_PORT_{port}");
    policies.iter().any(|policy| match policy {
        QosPolicySpec::Port {
            name, dscp: actual, ..
        } => {
            *actual == dscp
                && (name.eq_ignore_ascii_case(&current) || name.eq_ignore_ascii_case(&legacy))
        }
        QosPolicySpec::App { .. } => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TuningAction, TuningProfile};

    fn port(name: &str, dscp: u8) -> QosPolicySpec {
        QosPolicySpec::Port {
            name: name.into(),
            protocol: "UDP".into(),
            port: 0,
            dscp,
        }
    }

    fn config() -> TuningConfig {
        TuningConfig {
            action: TuningAction::Verify,
            profile: TuningProfile::Safe,
            udp_ports: vec![3074, 3478, 40000],
            ..TuningConfig::default()
        }
    }

    #[test]
    fn verification_accepts_current_and_legacy_port_names_case_insensitively() {
        let result = evaluate(
            &config(),
            &[
                port("network_lantern_qos_port_3074", 46),
                port("NDS_QOS_PORT_3478", 46),
                port("NETWORK_LANTERN_QOS_PORT_40000", 46),
            ],
            Some(true),
        );
        assert!(result.success);
        assert!(result.missing_ports.is_empty());
        assert_eq!(
            result.components.get(&TuningComponent::LocalQos),
            Some(&ComponentStatus::Ok)
        );
        assert_eq!(
            result.components.get(&TuningComponent::QosPortPolicies),
            Some(&ComponentStatus::Ok)
        );
        assert!(!result.components.contains_key(&TuningComponent::Manifest));
    }

    #[test]
    fn verification_reports_missing_ports_and_disabled_local_qos() {
        let result = evaluate(
            &config(),
            &[port("NETWORK_LANTERN_QOS_PORT_3074", 46)],
            Some(false),
        );
        assert!(!result.success);
        assert_eq!(result.missing_ports, vec![3478, 40000]);
        assert_eq!(
            result.components.get(&TuningComponent::LocalQos),
            Some(&ComponentStatus::Warn)
        );
        assert_eq!(
            result.components.get(&TuningComponent::QosPolicies),
            Some(&ComponentStatus::Warn)
        );
        assert!(result.warnings[0].contains("3478, 40000"));
    }

    #[test]
    fn verification_flags_policies_with_a_different_dscp() {
        let result = evaluate(
            &config(),
            &[
                port("NETWORK_LANTERN_QOS_PORT_3074", 34),
                port("NETWORK_LANTERN_QOS_PORT_3478", 46),
                port("NETWORK_LANTERN_QOS_PORT_40000", 46),
            ],
            Some(true),
        );
        assert!(!result.success);
        assert_eq!(result.missing_ports, vec![3074]);
    }

    #[test]
    fn unreadable_local_qos_is_unknown_and_unsuccessful() {
        let result = evaluate(&config(), &[], None);
        assert!(!result.success);
        assert_eq!(
            result.components.get(&TuningComponent::LocalQos),
            Some(&ComponentStatus::Unknown)
        );
    }
}
