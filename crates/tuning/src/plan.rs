use crate::{
    PowerPlan, Result, TuningAction, TuningConfig, TuningError, TuningPlan, TuningProfile,
    TuningStep,
};
use std::{collections::BTreeSet, path::Component};

const MAX_PORTS: usize = 100;
const MAX_APPS: usize = 100;
const MAX_PATH_BYTES: usize = 32 * 1024;

pub fn plan(config: &TuningConfig) -> Result<TuningPlan> {
    let mut config = config.clone();
    validate(&config)?;
    config.udp_ports.sort_unstable();
    config.udp_ports.dedup();
    config.app_paths.retain(|path| !path.as_os_str().is_empty());
    let mut seen = BTreeSet::new();
    config
        .app_paths
        .retain(|path| seen.insert(path.as_os_str().to_owned()));

    let mut steps = Vec::new();
    match config.action {
        TuningAction::Apply => {
            steps.extend([
                TuningStep::ValidateBackupDestination,
                TuningStep::WriteAndVerifyBackup,
                TuningStep::EnableLocalQos,
            ]);
            if !config.udp_ports.is_empty() {
                steps.push(TuningStep::ReconcilePortPolicies {
                    count: config.udp_ports.len(),
                    dscp: config.dscp,
                });
            }
            if config.include_app_policies && !config.app_paths.is_empty() {
                steps.push(TuningStep::ReconcileAppPolicies {
                    count: config.app_paths.len(),
                    dscp: config.dscp,
                });
            }
            if config.profile == TuningProfile::Measured {
                steps.push(TuningStep::DisableNicPowerSaving);
            }
            let effective_power = if config.power_plan == PowerPlan::None
                && config.profile == TuningProfile::Measured
            {
                PowerPlan::HighPerformance
            } else {
                config.power_plan
            };
            if effective_power != PowerPlan::None {
                steps.push(TuningStep::SetPowerPlan {
                    plan: effective_power,
                });
            }
        }
        TuningAction::Backup => steps.extend([
            TuningStep::ValidateBackupDestination,
            TuningStep::WriteAndVerifyBackup,
        ]),
        TuningAction::Restore => steps.extend([
            TuningStep::ValidateAndStageRestore,
            TuningStep::RestoreRegistry,
            TuningStep::RestoreQos,
            TuningStep::RestoreNicAdvanced,
            TuningStep::RestoreRsc,
            TuningStep::RestorePowerPlan,
        ]),
        TuningAction::Verify => steps.push(TuningStep::InspectManagedState),
    }
    Ok(TuningPlan {
        mutating: config.action != TuningAction::Verify && !config.dry_run,
        requires_helper: config.action != TuningAction::Verify && !config.dry_run,
        config,
        steps,
    })
}

fn validate(config: &TuningConfig) -> Result<()> {
    if config.dscp > 63 {
        return Err(TuningError::Validation("dscp must be in 0..=63".into()));
    }
    if config.udp_ports.len() > MAX_PORTS {
        return Err(TuningError::Validation(format!(
            "udpPorts exceeds {MAX_PORTS} entries"
        )));
    }
    if config.udp_ports.contains(&0) {
        return Err(TuningError::Validation(
            "udpPorts must be in 1..=65535".into(),
        ));
    }
    if config.app_paths.len() > MAX_APPS {
        return Err(TuningError::Validation(format!(
            "appPaths exceeds {MAX_APPS} entries"
        )));
    }
    for path in &config.app_paths {
        if path.as_os_str().as_encoded_bytes().len() > MAX_PATH_BYTES {
            return Err(TuningError::Validation(
                "application path is too long".into(),
            ));
        }
        if path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
            || path
                .as_os_str()
                .to_string_lossy()
                .split(['\\', '/'])
                .any(|part| part == "..")
        {
            return Err(TuningError::Validation(
                "application paths must not contain parent traversal".into(),
            ));
        }
        if !valid_windows_exe(path) {
            return Err(TuningError::Validation(
                "application paths must be absolute local .exe paths".into(),
            ));
        }
    }
    if config.action != TuningAction::Verify
        && config
            .backup_folder
            .as_ref()
            .is_some_and(|path| path.as_os_str().is_empty())
    {
        return Err(TuningError::Validation(
            "backupFolder must not be empty".into(),
        ));
    }
    if config.action != TuningAction::Verify
        && !config.allow_unsafe_backup_folder
        && config
            .backup_folder
            .as_ref()
            .is_some_and(|path| sensitive_backup_folder(path))
    {
        return Err(TuningError::Validation(
            "backupFolder points into Windows or Program Files; allowUnsafeBackupFolder is required".into(),
        ));
    }
    if config.action != TuningAction::Verify
        && config.backup_folder.as_ref().is_some_and(|path| {
            path.as_os_str()
                .to_string_lossy()
                .split(['\\', '/'])
                .any(|part| part == "..")
        })
    {
        return Err(TuningError::Validation(
            "backupFolder must not contain parent traversal".into(),
        ));
    }
    Ok(())
}

fn valid_windows_exe(path: &std::path::Path) -> bool {
    let value = path.as_os_str().to_string_lossy();
    value.len() >= 7
        && value.as_bytes()[0].is_ascii_alphabetic()
        && value.as_bytes().get(1) == Some(&b':')
        && matches!(value.as_bytes().get(2), Some(b'\\' | b'/'))
        && value.to_ascii_lowercase().ends_with(".exe")
        && !value.starts_with("\\\\")
        && !value.starts_with("\\\\?\\")
        && !value.contains('\0')
}

fn sensitive_backup_folder(path: &std::path::Path) -> bool {
    let normalized = path
        .as_os_str()
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase();
    [
        r"c:\windows",
        r"c:\windows\system32",
        r"c:\program files",
        r"c:\program files (x86)",
    ]
    .iter()
    .any(|root| normalized == *root || normalized.starts_with(&format!("{root}\\")))
        || normalized
            .split('\\')
            .any(|part| matches!(part, "windows" | "program files" | "program files (x86)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planning_is_pure_and_normalizes_collections() {
        let config = TuningConfig {
            udp_ports: vec![5201, 5201, 9000],
            app_paths: vec!["C:\\game.exe".into(), "C:\\game.exe".into()],
            include_app_policies: true,
            ..TuningConfig::default()
        };
        let result = plan(&config).unwrap();
        assert_eq!(result.config.udp_ports, [5201, 9000]);
        assert_eq!(result.config.app_paths.len(), 1);
        assert!(result.steps.contains(&TuningStep::WriteAndVerifyBackup));
    }

    #[test]
    fn measured_defaults_to_high_performance() {
        let result = plan(&TuningConfig {
            profile: TuningProfile::Measured,
            ..TuningConfig::default()
        })
        .unwrap();
        assert!(result.steps.contains(&TuningStep::SetPowerPlan {
            plan: PowerPlan::HighPerformance
        }));
    }

    #[test]
    fn rejects_invalid_provider_inputs_and_sensitive_backup_locations() {
        for config in [
            TuningConfig {
                udp_ports: vec![0],
                ..Default::default()
            },
            TuningConfig {
                include_app_policies: true,
                app_paths: vec!["relative.exe".into()],
                ..Default::default()
            },
            TuningConfig {
                backup_folder: Some(r"C:\Windows\Temp\Lantern".into()),
                ..Default::default()
            },
            TuningConfig {
                app_paths: vec![r"C:\Apps\..\escape.exe".into()],
                ..Default::default()
            },
            TuningConfig {
                backup_folder: Some(r"C:\ProgramData\safe\..\other".into()),
                ..Default::default()
            },
        ] {
            assert!(matches!(plan(&config), Err(TuningError::Validation(_))));
        }
        assert!(
            plan(&TuningConfig {
                backup_folder: Some(r"C:\Windows\Temp\Lantern".into()),
                allow_unsafe_backup_folder: true,
                ..Default::default()
            })
            .is_ok()
        );
    }
}
