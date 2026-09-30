use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TuningAction {
    #[default]
    Apply,
    Backup,
    Restore,
    Verify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TuningProfile {
    #[default]
    Safe,
    Measured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PowerPlan {
    #[default]
    None,
    HighPerformance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TuningConfig {
    pub action: TuningAction,
    #[serde(alias = "tuningProfile")]
    pub profile: TuningProfile,
    pub dscp: u8,
    pub udp_ports: Vec<u16>,
    pub include_app_policies: bool,
    pub app_paths: Vec<PathBuf>,
    pub power_plan: PowerPlan,
    pub backup_folder: Option<PathBuf>,
    pub allow_unsafe_backup_folder: bool,
    pub dry_run: bool,
}

impl Default for TuningConfig {
    fn default() -> Self {
        Self {
            action: TuningAction::Apply,
            profile: TuningProfile::Safe,
            dscp: 46,
            udp_ports: Vec::new(),
            include_app_policies: false,
            app_paths: Vec::new(),
            power_plan: PowerPlan::None,
            backup_folder: None,
            allow_unsafe_backup_folder: false,
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TuningComponent {
    Backup,
    Manifest,
    Registry,
    LocalQos,
    QosPolicies,
    QosPortPolicies,
    QosAppPolicies,
    NicAdvanced,
    NicPowerSaving,
    Rsc,
    PowerPlan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComponentStatus {
    #[serde(rename = "OK")]
    Ok,
    Warn,
    Skipped,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TuningStep {
    ValidateBackupDestination,
    WriteAndVerifyBackup,
    EnableLocalQos,
    ReconcilePortPolicies { count: usize, dscp: u8 },
    ReconcileAppPolicies { count: usize, dscp: u8 },
    DisableNicPowerSaving,
    SetPowerPlan { plan: PowerPlan },
    ValidateAndStageRestore,
    RestoreRegistry,
    RestoreQos,
    RestoreNicAdvanced,
    RestoreRsc,
    RestorePowerPlan,
    InspectManagedState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TuningPlan {
    pub config: TuningConfig,
    pub steps: Vec<TuningStep>,
    pub mutating: bool,
    pub requires_helper: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TuningResult {
    pub action: TuningAction,
    pub tuning_profile: Option<TuningProfile>,
    pub dscp: u8,
    pub udp_ports: Vec<u16>,
    pub include_app_policies: bool,
    pub app_paths: Vec<PathBuf>,
    pub dry_run: bool,
    pub success: bool,
    pub backup_folder: Option<PathBuf>,
    pub managed_policies: Vec<String>,
    pub missing_ports: Vec<u16>,
    pub components: BTreeMap<TuningComponent, ComponentStatus>,
    pub warnings: Vec<String>,
    pub timestamp: DateTime<Utc>,
}

impl TuningResult {
    pub(crate) fn empty(config: &TuningConfig) -> Self {
        Self {
            action: config.action,
            tuning_profile: (config.action == TuningAction::Apply).then_some(config.profile),
            dscp: config.dscp,
            udp_ports: config.udp_ports.clone(),
            include_app_policies: config.include_app_policies,
            app_paths: config.app_paths.clone(),
            dry_run: config.dry_run,
            success: true,
            backup_folder: config.backup_folder.clone(),
            managed_policies: Vec::new(),
            missing_ports: Vec::new(),
            components: BTreeMap::new(),
            warnings: Vec::new(),
            timestamp: Utc::now(),
        }
    }
}
