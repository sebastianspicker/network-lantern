use crate::{ComponentStatus, TuningComponent, TuningConfig, TuningResult};

pub(crate) fn evaluate(
    config: &TuningConfig,
    managed_policies: Vec<String>,
    local_qos_enabled: Option<bool>,
) -> TuningResult {
    let mut result = TuningResult::empty(config);
    result.managed_policies = managed_policies;
    result.missing_ports = config
        .udp_ports
        .iter()
        .copied()
        .filter(|port| !has_port_policy(&result.managed_policies, *port))
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
            "Missing managed QoS port policies for: {}",
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

fn has_port_policy(names: &[String], port: u16) -> bool {
    let current = format!("NETWORK_LANTERN_QOS_PORT_{port}");
    let legacy = format!("NDS_QOS_PORT_{port}");
    names
        .iter()
        .any(|name| name.eq_ignore_ascii_case(&current) || name.eq_ignore_ascii_case(&legacy))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TuningAction, TuningProfile};

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
            vec![
                "network_lantern_qos_port_3074".into(),
                "NDS_QOS_PORT_3478".into(),
                "NETWORK_LANTERN_QOS_PORT_40000".into(),
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
            vec!["NETWORK_LANTERN_QOS_PORT_3074".into()],
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
    fn unreadable_local_qos_is_unknown_and_unsuccessful() {
        let result = evaluate(&config(), Vec::new(), None);
        assert!(!result.success);
        assert_eq!(
            result.components.get(&TuningComponent::LocalQos),
            Some(&ComponentStatus::Unknown)
        );
    }
}
