mod wmi;

use self::wmi::{Session, Value};
use crate::{
    ArtifactKind, BackupManifest, ComponentStatus, PowerPlan, Result, StagedBackup, TuningAction,
    TuningComponent, TuningError, TuningPlan, TuningResult,
    backup::{
        BackupManifestComponents, NicAdvancedRow, QosPolicySpec, RegistryValue, RscRow,
        persist::{self, DurableFile},
        publication::{self, PublicationFs},
        sha256_upper,
        trusted_io::{self, FileIdentity, TrustedDirectory},
    },
    validate_backup_bundle,
};
use chrono::Utc;
use std::{
    collections::{BTreeMap, HashSet},
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::Write,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;
use windows_sys::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, LocalFree},
        Security::{
            Authorization::{
                ConvertSecurityDescriptorToStringSecurityDescriptorW,
                ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW,
                SE_FILE_OBJECT, SetNamedSecurityInfoW,
            },
            DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, GetSecurityDescriptorControl,
            GetSecurityDescriptorGroup, GetSecurityDescriptorOwner, OWNER_SECURITY_INFORMATION,
            PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED,
        },
        System::{
            Power::{PowerGetActiveScheme, PowerSetActiveScheme},
            Registry::{
                HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_64KEY,
                REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW,
                RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
            },
        },
    },
    core::GUID,
};

const SYSTEM_PROFILE: &str =
    "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile";
const AUDIO_TASK: &str =
    "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile\\Tasks\\Audio";
const AFD_PARAMETERS: &str = "SYSTEM\\CurrentControlSet\\Services\\AFD\\Parameters";
const TCPIP_QOS: &str = "SYSTEM\\CurrentControlSet\\Services\\Tcpip\\QoS";
const HIGH_PERFORMANCE: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
const MANAGED_PREFIXES: &[&str] = &[
    "NETWORK_LANTERN_QOS_PORT_",
    "NETWORK_LANTERN_QOS_APP_",
    "NDS_QOS_PORT_",
    "NDS_QOS_APP_",
    "QoS_UDP_TS_",
    "QoS_UDP_CS2_",
    "QoS_APP_",
];

pub(super) async fn execute(
    plan: &TuningPlan,
    cancellation: &CancellationToken,
    inspect_only: bool,
) -> Result<TuningResult> {
    let plan = plan.clone();
    let cancellation = cancellation.clone();
    tokio::task::spawn_blocking(move || execute_sync(&plan, &cancellation, inspect_only))
        .await
        .map_err(|error| TuningError::Native {
            operation: "join Windows tuning worker",
            code: -1,
            message: error.to_string(),
        })?
}

fn execute_sync(
    plan: &TuningPlan,
    cancellation: &CancellationToken,
    inspect_only: bool,
) -> Result<TuningResult> {
    check_cancel(cancellation)?;
    let cim = Session::connect()?;
    crate::backup::provider::after_inventory(
        || read_qos(&cim),
        |current_qos| {
            execute_after_qos_inventory(plan, cancellation, inspect_only, &cim, current_qos)
        },
    )
}

fn execute_after_qos_inventory(
    plan: &TuningPlan,
    cancellation: &CancellationToken,
    inspect_only: bool,
    cim: &Session,
    current_qos: Vec<QosPolicySpec>,
) -> Result<TuningResult> {
    let managed_policies = current_qos
        .iter()
        .map(|policy| policy.name().into())
        .collect::<Vec<_>>();
    if inspect_only || plan.config.action == TuningAction::Verify {
        let mut verification = crate::verification::evaluate(
            &plan.config,
            managed_policies,
            inspect_local_qos_enabled(),
        );
        verification.backup_folder = Some(backup_folder(plan)?);
        return Ok(verification);
    }
    let mut result = TuningResult::empty(&plan.config);
    result.managed_policies = managed_policies;

    let folder = backup_folder(plan)?;
    match plan.config.action {
        TuningAction::Backup => {
            let components = publish_backup(&folder, cim, &current_qos, cancellation)?;
            record_partial_backup(&components, &mut result);
            result
                .components
                .insert(TuningComponent::Backup, ComponentStatus::Ok);
        }
        TuningAction::Apply => {
            // QoS inventory succeeded before any registry, provider, or power mutation.
            let components = publish_backup(&folder, cim, &current_qos, cancellation)?;
            record_partial_backup(&components, &mut result);
            result
                .components
                .insert(TuningComponent::Backup, ComponentStatus::Ok);
            check_cancel(cancellation)?;
            set_registry_string(TCPIP_QOS, "Do not use NLA", "1")?;
            result
                .components
                .insert(TuningComponent::LocalQos, ComponentStatus::Ok);
            let desired = desired_qos(plan);
            reconcile_qos(cim, &current_qos, &desired)?;
            result.managed_policies = desired.iter().map(|policy| policy.name().into()).collect();
            result
                .components
                .insert(TuningComponent::QosPolicies, ComponentStatus::Ok);
            if plan.config.profile == crate::TuningProfile::Measured {
                disable_nic_power_saving(cim)?;
                result
                    .components
                    .insert(TuningComponent::NicPowerSaving, ComponentStatus::Ok);
            }
            if plan.config.power_plan == PowerPlan::HighPerformance
                || plan.config.profile == crate::TuningProfile::Measured
            {
                set_power_plan(HIGH_PERFORMANCE)?;
                result
                    .components
                    .insert(TuningComponent::PowerPlan, ComponentStatus::Ok);
            }
        }
        TuningAction::Restore => {
            let bundle = validate_backup_bundle(&folder)?;
            let staged = bundle.stage_verified()?;
            let quarantine = folder.with_file_name(format!(
                "NetworkLantern-quarantine-{}",
                uuid::Uuid::new_v4().simple()
            ));
            if quarantine.exists() {
                return Err(TuningError::BackupInvalid(
                    "restore quarantine path collision".into(),
                ));
            }
            publish_backup(&quarantine, cim, &current_qos, cancellation)?;
            let restore = restore_staged(&staged, cim, cancellation, &mut result);
            if let Err(error) = restore {
                let rollback = validate_backup_bundle(&quarantine)
                    .and_then(|bundle| bundle.stage_verified())
                    .and_then(|staged| {
                        restore_staged(
                            &staged,
                            cim,
                            &CancellationToken::new(),
                            &mut TuningResult::empty(&plan.config),
                        )
                    });
                return Err(TuningError::restore_recovery(error, quarantine, rollback));
            }
            let quarantine_identity = trusted_io::directory_identity(&quarantine)?;
            validate_backup_bundle(&quarantine)?;
            trusted_io::remove_owned_directory(&quarantine, quarantine_identity)?;
        }
        TuningAction::Verify => unreachable!(),
    }
    result.backup_folder = Some(folder);
    Ok(result)
}

fn restore_staged(
    staged: &StagedBackup,
    cim: &Session,
    cancellation: &CancellationToken,
    result: &mut TuningResult,
) -> Result<()> {
    staged.verify()?;
    for kind in staged.expected_artifacts() {
        check_cancel(cancellation)?;
        staged.verify()?;
        match kind {
            ArtifactKind::SystemProfile => restore_registry(&staged.read(kind)?, true)?,
            ArtifactKind::AfdParameters => restore_registry(&staged.read(kind)?, false)?,
            ArtifactKind::QosPolicies => {
                let desired = crate::backup::parse_qos_clixml(&staged.read(kind)?)?;
                let current = read_qos(cim)?;
                reconcile_qos(cim, &current, &desired)?;
            }
            ArtifactKind::NicAdvanced => {
                restore_nic(cim, &crate::backup::parse_nic_csv(&staged.read(kind)?)?)?
            }
            ArtifactKind::NicRsc => {
                restore_rsc(cim, &crate::backup::parse_rsc_csv(&staged.read(kind)?)?)?
            }
            ArtifactKind::PowerPlan => {
                let bytes = staged.read(kind)?;
                let value = std::str::from_utf8(&bytes)
                    .map_err(|_| TuningError::BackupInvalid("power plan is not UTF-8".into()))?;
                set_power_plan(value.trim())?;
            }
        }
        result
            .components
            .insert(component(kind), ComponentStatus::Ok);
    }
    Ok(())
}

fn component(kind: ArtifactKind) -> TuningComponent {
    match kind {
        ArtifactKind::SystemProfile | ArtifactKind::AfdParameters => TuningComponent::Registry,
        ArtifactKind::QosPolicies => TuningComponent::QosPolicies,
        ArtifactKind::NicAdvanced => TuningComponent::NicAdvanced,
        ArtifactKind::NicRsc => TuningComponent::Rsc,
        ArtifactKind::PowerPlan => TuningComponent::PowerPlan,
    }
}

fn backup_folder(plan: &TuningPlan) -> Result<PathBuf> {
    if let Some(folder) = &plan.config.backup_folder {
        return Ok(folder.clone());
    }
    let base = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    let current = base.join("NetworkLantern");
    let legacy = base.join("NetworkDiagnosticsSuite");
    if plan.config.action == TuningAction::Restore && !current.exists() && legacy.exists() {
        Ok(legacy)
    } else {
        Ok(current)
    }
}

fn publish_backup(
    folder: &Path,
    cim: &Session,
    qos: &[QosPolicySpec],
    cancellation: &CancellationToken,
) -> Result<BackupManifestComponents> {
    check_cancel(cancellation)?;
    let parent = folder
        .parent()
        .ok_or_else(|| TuningError::BackupInvalid("backup folder has no parent".into()))?;
    fs::create_dir_all(parent).map_err(|source| TuningError::io("create backup parent", source))?;
    crate::native::validate_backup_parent(parent)?;
    let staging = parent.join(format!(
        ".network-lantern-backup-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&staging).map_err(|source| TuningError::io("create backup staging", source))?;
    let mut staging_guard = OwnedPublicationTree::new(staging.clone())?;
    protect_directory(&staging)?;

    let result = (|| {
        let mut artifacts = BTreeMap::new();
        artifacts.insert(
            ArtifactKind::SystemProfile,
            registry_export(true)?.into_bytes(),
        );
        artifacts.insert(
            ArtifactKind::AfdParameters,
            registry_export(false)?.into_bytes(),
        );
        artifacts.insert(ArtifactKind::QosPolicies, qos_xml(qos).into_bytes());
        let physical_up = physical_up_adapters(cim)?;
        let nic = nic_csv(cim, &physical_up)?;
        let rsc = rsc_csv(cim, &physical_up)?;
        if !nic.is_empty() {
            artifacts.insert(ArtifactKind::NicAdvanced, nic);
        }
        if !rsc.is_empty() {
            artifacts.insert(ArtifactKind::NicRsc, rsc);
        }
        artifacts.insert(ArtifactKind::PowerPlan, active_power_plan()?.into_bytes());
        let mut digests = BTreeMap::new();
        for (kind, bytes) in &artifacts {
            check_cancel(cancellation)?;
            write_protected(&staging.join(kind.file_name()), bytes)?;
            digests.insert(kind.file_name().into(), sha256_upper(bytes));
        }
        let components = BackupManifestComponents {
            system_profile: true,
            afd_parameters: true,
            qos_policies: true,
            nic_advanced: artifacts.contains_key(&ArtifactKind::NicAdvanced),
            nic_rsc: artifacts.contains_key(&ArtifactKind::NicRsc),
            power_plan: true,
        };
        let manifest = BackupManifest {
            schema_version: 3,
            tool_name: "network-lantern".into(),
            machine_name: std::env::var("COMPUTERNAME").ok(),
            platform: Some("Win32NT".into()),
            os_version: Some(std::env::var("OS").unwrap_or_else(|_| "Windows_NT".into())),
            module_version: Some(env!("CARGO_PKG_VERSION").into()),
            timestamp: Some(Utc::now().to_rfc3339()),
            components: components.clone(),
            artifact_digests: digests,
        };
        write_protected(
            &staging.join("backup_manifest.json"),
            &serde_json::to_vec_pretty(&manifest)
                .map_err(|error| TuningError::BackupInvalid(format!("encode manifest: {error}")))?,
        )?;
        TrustedDirectory::open(&staging, "open backup staging for flush")?.sync()?;
        validate_backup_bundle(&staging)?;
        Ok(components)
    })();
    let components = result?;
    let previous = parent.join(format!(
        ".network-lantern-previous-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let quarantine = parent.join(format!(
        ".network-lantern-failed-{}",
        uuid::Uuid::new_v4().simple()
    ));
    publication::replace_verified(
        &WindowsPublicationFs,
        parent,
        &staging,
        folder,
        &previous,
        &quarantine,
    )?;
    staging_guard.disarm();
    Ok(components)
}

fn record_partial_backup(components: &BackupManifestComponents, result: &mut TuningResult) {
    if !components.nic_advanced {
        result
            .warnings
            .push("backup contains no physical, up-adapter advanced properties".into());
    }
    if !components.nic_rsc {
        result
            .warnings
            .push("backup contains no physical, up-adapter RSC state".into());
    }
}

fn read_qos(cim: &Session) -> Result<Vec<QosPolicySpec>> {
    let rows = cim.query(
        "MSFT_NetQosPolicySettingData",
        &[
            "__PATH",
            "Name",
            "IPProtocolMatchCondition",
            "IPPortMatchCondition",
            "AppPathNameMatchCondition",
            "DSCPAction",
        ],
        None,
    )?;
    let mut policies = Vec::new();
    for row in rows {
        if let Some(policy) = parse_qos_row(&row)? {
            policies.push(policy);
        }
    }
    Ok(policies)
}

fn reconcile_qos(
    cim: &Session,
    current: &[QosPolicySpec],
    desired: &[QosPolicySpec],
) -> Result<()> {
    let rows = cim.query(
        "MSFT_NetQosPolicySettingData",
        &[
            "__PATH",
            "Name",
            "IPProtocolMatchCondition",
            "IPPortMatchCondition",
            "AppPathNameMatchCondition",
            "DSCPAction",
        ],
        None,
    )?;
    let mut managed = Vec::new();
    for row in rows {
        if parse_qos_row(&row)?.is_some() {
            managed.push(row);
        }
    }
    for row in &managed {
        cim.delete(row)?;
    }
    if let Err(error) = desired
        .iter()
        .try_for_each(|policy| create_qos(cim, policy))
    {
        let _ = cim
            .query("MSFT_NetQosPolicySettingData", &["__PATH", "Name"], None)
            .map(|rows| {
                rows.into_iter()
                    .filter(|row| row.string("Name").is_some_and(managed_name))
                    .for_each(|row| {
                        let _ = cim.delete(&row);
                    })
            });
        let rollback = current
            .iter()
            .try_for_each(|policy| create_qos(cim, policy));
        if rollback.is_err() {
            return Err(TuningError::Native {
                operation: "rollback QoS policies",
                code: -1,
                message: format!("mutation failed: {error}; rollback failed"),
            });
        }
        return Err(error);
    }
    Ok(())
}

fn parse_qos_row(row: &wmi::Row) -> Result<Option<QosPolicySpec>> {
    let Some(name) = row.string("Name").filter(|name| !name.trim().is_empty()) else {
        return Err(provider_inventory("QoS provider row is missing Name"));
    };
    if !managed_name(name) {
        return Ok(None);
    }
    if row
        .string("__PATH")
        .filter(|path| !path.trim().is_empty())
        .is_none()
    {
        return Err(provider_inventory(
            "managed QoS provider row is missing __PATH",
        ));
    }
    let dscp = row
        .number("DSCPAction")
        .and_then(|value| u8::try_from(value).ok())
        .filter(|value| *value <= 63)
        .ok_or_else(|| {
            provider_inventory("managed QoS policy has invalid or missing DSCPAction")
        })?;
    let app = row
        .string("AppPathNameMatchCondition")
        .filter(|value| !value.is_empty());
    let port = row.number("IPPortMatchCondition");
    let protocol = row.number("IPProtocolMatchCondition");
    match (app, port, protocol) {
        (Some(path), None, None) if valid_windows_exe(path) => {
            Ok(Some(QosPolicySpec::app(name.into(), path.into(), dscp)))
        }
        (Some(_), _, _) => Err(provider_inventory(
            "managed application QoS policy has an invalid path or conflicting match conditions",
        )),
        (None, Some(port), Some(protocol)) => {
            let port = u16::try_from(port)
                .ok()
                .filter(|value| *value != 0)
                .ok_or_else(|| provider_inventory("managed QoS policy has invalid port"))?;
            let protocol = match protocol {
                1 => "TCP",
                2 => "UDP",
                _ => {
                    return Err(provider_inventory(
                        "managed QoS policy protocol must be TCP or UDP",
                    ));
                }
            };
            Ok(Some(QosPolicySpec::port(
                name.into(),
                protocol.into(),
                port,
                dscp,
            )))
        }
        _ => Err(provider_inventory(
            "managed QoS policy has incomplete match conditions",
        )),
    }
}

fn provider_inventory(message: &str) -> TuningError {
    TuningError::Native {
        operation: "read QoS inventory",
        code: -1,
        message: message.into(),
    }
}

fn valid_windows_exe(path: &str) -> bool {
    path.len() >= 7
        && path.as_bytes().get(1) == Some(&b':')
        && path.as_bytes().get(2) == Some(&b'\\')
        && path.to_ascii_lowercase().ends_with(".exe")
        && !path.starts_with("\\\\")
        && !path.starts_with("\\\\?\\")
        && !path.contains('\0')
}

fn create_qos(cim: &Session, policy: &QosPolicySpec) -> Result<()> {
    let mut values = vec![
        ("Name", Value::String(policy.name().into())),
        ("NetworkProfile", Value::Number(0)),
    ];
    if let Some((protocol, port, dscp)) = policy.port_parts() {
        values.push((
            "IPProtocolMatchCondition",
            Value::Number(if protocol == "TCP" { 1 } else { 2 }),
        ));
        values.push(("IPPortMatchCondition", Value::Number(i64::from(port))));
        values.push(("DSCPAction", Value::Number(i64::from(dscp))));
    } else if let Some((app_path, dscp)) = policy.app_parts() {
        values.push(("AppPathNameMatchCondition", Value::String(app_path.into())));
        values.push(("DSCPAction", Value::Number(i64::from(dscp))));
    }
    cim.create("MSFT_NetQosPolicySettingData", &values)
}

fn desired_qos(plan: &TuningPlan) -> Vec<QosPolicySpec> {
    let mut values = plan
        .config
        .udp_ports
        .iter()
        .map(|port| {
            QosPolicySpec::port(
                format!("NETWORK_LANTERN_QOS_PORT_{port}"),
                "UDP".into(),
                *port,
                plan.config.dscp,
            )
        })
        .collect::<Vec<_>>();
    if plan.config.include_app_policies {
        values.extend(
            plan.config
                .app_paths
                .iter()
                .enumerate()
                .map(|(index, path)| {
                    QosPolicySpec::app(
                        format!("NETWORK_LANTERN_QOS_APP_{index}"),
                        path.to_string_lossy().into(),
                        plan.config.dscp,
                    )
                }),
        );
    }
    values
}

fn disable_nic_power_saving(cim: &Session) -> Result<()> {
    let adapters = physical_up_adapters(cim)?;
    let rows = cim.query(
        "MSFT_NetAdapterAdvancedPropertySettingData",
        &["__PATH", "Name", "RegistryKeyword", "RegistryValue"],
        None,
    )?;
    let keywords: HashSet<&str> = ["*EEE", "*GreenEthernet", "*PowerSavingMode"]
        .into_iter()
        .collect();
    for row in rows {
        if row
            .string("Name")
            .is_some_and(|name| adapters.contains(name))
            && row
                .string("RegistryKeyword")
                .is_some_and(|value| keywords.contains(value))
        {
            cim.update(&row, &[("RegistryValue", Value::Strings(vec!["0".into()]))])?;
        }
    }
    Ok(())
}

fn restore_nic(cim: &Session, desired: &[NicAdvancedRow]) -> Result<()> {
    let rows = cim.query(
        "MSFT_NetAdapterAdvancedPropertySettingData",
        &["__PATH", "Name", "DisplayName", "RegistryKeyword"],
        None,
    )?;
    for value in desired {
        let row = rows
            .iter()
            .find(|row| {
                row.string("Name") == Some(value.adapter.as_str())
                    && ((!value.registry_keyword.is_empty()
                        && row.string("RegistryKeyword") == Some(value.registry_keyword.as_str()))
                        || (!value.display_name.is_empty()
                            && row.string("DisplayName") == Some(value.display_name.as_str())))
            })
            .ok_or_else(|| TuningError::Native {
                operation: "restore NIC property",
                code: -1,
                message: format!("property missing for {}", value.adapter),
            })?;
        if !value.registry_keyword.is_empty() {
            cim.update(
                row,
                &[(
                    "RegistryValue",
                    Value::Strings(vec![value.registry_value.clone()]),
                )],
            )?;
        } else {
            cim.update(
                row,
                &[("DisplayValue", Value::String(value.display_value.clone()))],
            )?;
        }
    }
    Ok(())
}

fn restore_rsc(cim: &Session, desired: &[RscRow]) -> Result<()> {
    let rows = cim.query("MSFT_NetAdapterRscSettingData", &["__PATH", "Name"], None)?;
    for value in desired {
        let row = rows
            .iter()
            .find(|row| row.string("Name") == Some(value.name.as_str()))
            .ok_or_else(|| TuningError::Native {
                operation: "restore RSC",
                code: -1,
                message: format!("adapter missing: {}", value.name),
            })?;
        if value.ipv4_enabled || value.ipv6_enabled {
            cim.invoke(
                row,
                "MSFT_NetAdapterRscSettingData",
                "Enable",
                &[
                    ("IPv4", Value::Bool(value.ipv4_enabled)),
                    ("IPv6", Value::Bool(value.ipv6_enabled)),
                ],
            )?;
        }
        if !value.ipv4_enabled || !value.ipv6_enabled {
            cim.invoke(
                row,
                "MSFT_NetAdapterRscSettingData",
                "Disable",
                &[
                    ("IPv4", Value::Bool(!value.ipv4_enabled)),
                    ("IPv6", Value::Bool(!value.ipv6_enabled)),
                ],
            )?;
        }
    }
    Ok(())
}

fn physical_up_adapters(cim: &Session) -> Result<HashSet<String>> {
    let rows = cim.query(
        "MSFT_NetAdapter",
        &["Name", "HardwareInterface", "Status"],
        Some("HardwareInterface = TRUE AND Status = 'Up'"),
    )?;
    rows.into_iter()
        .map(|row| {
            row.string("Name")
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| TuningError::Native {
                    operation: "read physical adapter inventory",
                    code: -1,
                    message: "provider returned an adapter without a name".into(),
                })
        })
        .collect()
}

fn nic_csv(cim: &Session, adapters: &HashSet<String>) -> Result<Vec<u8>> {
    let rows = cim.query(
        "MSFT_NetAdapterAdvancedPropertySettingData",
        &[
            "Name",
            "DisplayName",
            "RegistryKeyword",
            "DisplayValue",
            "RegistryValue",
        ],
        None,
    )?;
    let rows = rows
        .into_iter()
        .filter(|row| {
            row.string("Name")
                .is_some_and(|name| adapters.contains(name))
        })
        .take(512)
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let mut writer = csv::Writer::from_writer(Vec::new());
    for row in rows {
        writer
            .serialize(NicAdvancedRow {
                adapter: row.string("Name").unwrap_or_default().into(),
                display_name: row.string("DisplayName").unwrap_or_default().into(),
                registry_keyword: row.string("RegistryKeyword").unwrap_or_default().into(),
                display_value: row.string("DisplayValue").unwrap_or_default().into(),
                registry_value: row
                    .strings("RegistryValue")
                    .and_then(|values| values.first())
                    .cloned()
                    .unwrap_or_default(),
            })
            .map_err(|error| TuningError::BackupInvalid(format!("encode NIC CSV: {error}")))?;
    }
    writer
        .into_inner()
        .map_err(|error| TuningError::BackupInvalid(format!("finish NIC CSV: {error}")))
}

fn rsc_csv(cim: &Session, adapters: &HashSet<String>) -> Result<Vec<u8>> {
    let rows = cim.query(
        "MSFT_NetAdapterRscSettingData",
        &["Name", "IPv4Enabled", "IPv6Enabled"],
        None,
    )?;
    let rows = rows
        .into_iter()
        .filter(|row| {
            row.string("Name")
                .is_some_and(|name| adapters.contains(name))
        })
        .take(64)
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let mut writer = csv::Writer::from_writer(Vec::new());
    for row in rows {
        writer
            .serialize(RscRow {
                name: row.string("Name").unwrap_or_default().into(),
                ipv4_enabled: row.boolean("IPv4Enabled").unwrap_or(false),
                ipv6_enabled: row.boolean("IPv6Enabled").unwrap_or(false),
            })
            .map_err(|error| TuningError::BackupInvalid(format!("encode RSC CSV: {error}")))?;
    }
    writer
        .into_inner()
        .map_err(|error| TuningError::BackupInvalid(format!("finish RSC CSV: {error}")))
}

fn qos_xml(policies: &[QosPolicySpec]) -> String {
    let mut xml = String::from("<Objs><LST>");
    for policy in policies {
        xml.push_str("<Obj><MS>");
        xml_field(&mut xml, "Name", policy.name());
        if let Some((protocol, port, dscp)) = policy.port_parts() {
            xml_field(&mut xml, "Type", "Port");
            xml_field(&mut xml, "Protocol", protocol);
            xml_field(&mut xml, "Port", &port.to_string());
            xml_field(&mut xml, "Dscp", &dscp.to_string());
        } else if let Some((app_path, dscp)) = policy.app_parts() {
            xml_field(&mut xml, "Type", "App");
            xml_field(&mut xml, "AppPath", app_path);
            xml_field(&mut xml, "Dscp", &dscp.to_string());
        }
        xml.push_str("</MS></Obj>");
    }
    xml.push_str("</LST></Objs>");
    xml
}

fn xml_field(output: &mut String, name: &str, value: &str) {
    output.push_str("<S N=\"");
    output.push_str(name);
    output.push_str("\">");
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&apos;"),
            value => output.push(value),
        }
    }
    output.push_str("</S>");
}

fn managed_name(name: &str) -> bool {
    MANAGED_PREFIXES.iter().any(|prefix| {
        name.len() >= prefix.len() && name[..prefix.len()].eq_ignore_ascii_case(prefix)
    })
}

fn registry_export(system: bool) -> Result<String> {
    let groups: &[(&str, &[(&str, bool)])] = if system {
        &[
            (
                SYSTEM_PROFILE,
                &[
                    ("SystemResponsiveness", false),
                    ("NetworkThrottlingIndex", false),
                ],
            ),
            (
                AUDIO_TASK,
                &[
                    ("Priority", false),
                    ("BackgroundOnly", false),
                    ("Clock Rate", false),
                    ("SchedulingCategory", true),
                    ("SFIOPriority", true),
                ],
            ),
        ]
    } else {
        &[(AFD_PARAMETERS, &[("FastSendDatagramThreshold", false)])]
    };
    let mut output = String::from("Windows Registry Editor Version 5.00\r\n");
    for (key, values) in groups {
        output.push_str(&format!("\r\n[HKEY_LOCAL_MACHINE\\{key}]\r\n"));
        for (name, string) in *values {
            match read_registry(key, name, *string)? {
                RegistryValue::Missing => output.push_str(&format!("\"{name}\"=-\r\n")),
                RegistryValue::Dword(value) => {
                    output.push_str(&format!("\"{name}\"=dword:{value:08x}\r\n"))
                }
                RegistryValue::String(value) => output.push_str(&format!(
                    "\"{name}\"=\"{}\"\r\n",
                    value.replace('\\', "\\\\").replace('"', "\\\"")
                )),
            }
        }
    }
    Ok(output)
}

fn restore_registry(bytes: &[u8], system: bool) -> Result<()> {
    let values = crate::backup::parse_registry_backup(bytes, system)?;
    for (full_key, entries) in values {
        let key = full_key
            .strip_prefix("HKEY_LOCAL_MACHINE\\")
            .ok_or_else(|| TuningError::BackupInvalid("registry key outside HKLM".into()))?;
        for (name, value) in entries {
            match value {
                RegistryValue::Missing => delete_registry_value(key, &name)?,
                RegistryValue::Dword(value) => set_registry_dword(key, &name, value)?,
                RegistryValue::String(value) => set_registry_string(key, &name, &value)?,
            }
        }
    }
    Ok(())
}

struct RegistryKey(HKEY);
impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn open_registry(path: &str, write: bool, create: bool) -> Result<RegistryKey> {
    let mut key = std::ptr::null_mut();
    let access = KEY_WOW64_64KEY
        | if write {
            KEY_SET_VALUE | KEY_QUERY_VALUE
        } else {
            KEY_QUERY_VALUE
        };
    let status = unsafe {
        if create {
            RegCreateKeyExW(
                HKEY_LOCAL_MACHINE,
                wide(path).as_ptr(),
                0,
                std::ptr::null_mut(),
                REG_OPTION_NON_VOLATILE,
                access,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        } else {
            RegOpenKeyExW(HKEY_LOCAL_MACHINE, wide(path).as_ptr(), 0, access, &mut key)
        }
    };
    if status != ERROR_SUCCESS {
        return Err(win32("open registry key", status));
    }
    Ok(RegistryKey(key))
}

fn read_registry(path: &str, name: &str, string: bool) -> Result<RegistryValue> {
    let Some((kind, bytes)) = read_registry_raw(path, name)? else {
        return Ok(RegistryValue::Missing);
    };
    if string && kind == REG_SZ && bytes.len().is_multiple_of(2) {
        let words = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return Ok(RegistryValue::String(
            String::from_utf16_lossy(&words)
                .trim_end_matches('\0')
                .into(),
        ));
    }
    if !string && kind == REG_DWORD && bytes.len() >= 4 {
        return Ok(RegistryValue::Dword(u32::from_le_bytes(
            bytes[..4].try_into().expect("length checked"),
        )));
    }
    Err(TuningError::BackupInvalid(format!(
        "registry value {name} has an unexpected type"
    )))
}

fn read_registry_raw(path: &str, name: &str) -> Result<Option<(u32, Vec<u8>)>> {
    let key = match open_registry(path, false, false) {
        Ok(key) => key,
        Err(TuningError::Native { code, .. }) if code == i64::from(ERROR_FILE_NOT_FOUND) => {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let mut kind = 0;
    let mut size = 0;
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            wide(name).as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS {
        return Err(win32("query registry value", status));
    }
    if size > 64 * 1024 {
        return Err(TuningError::BackupInvalid(format!(
            "registry value {name} exceeds 64 KiB"
        )));
    }
    let mut bytes = vec![0_u8; size as usize];
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            wide(name).as_ptr(),
            std::ptr::null(),
            &mut kind,
            bytes.as_mut_ptr(),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(win32("read registry value", status));
    }
    bytes.truncate(size as usize);
    Ok(Some((kind, bytes)))
}

fn inspect_local_qos_enabled() -> Option<bool> {
    match read_registry_raw(TCPIP_QOS, "Do not use NLA") {
        Ok(Some((REG_SZ, bytes))) if bytes.len().is_multiple_of(2) => {
            let words = bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>();
            Some(String::from_utf16_lossy(&words).trim_end_matches('\0') == "1")
        }
        Ok(Some((REG_DWORD, bytes))) if bytes.len() >= 4 => {
            Some(u32::from_le_bytes(bytes[..4].try_into().expect("length checked")) == 1)
        }
        Ok(_) | Err(_) => None,
    }
}

fn set_registry_dword(path: &str, name: &str, value: u32) -> Result<()> {
    let key = open_registry(path, true, true)?;
    let bytes = value.to_le_bytes();
    let status =
        unsafe { RegSetValueExW(key.0, wide(name).as_ptr(), 0, REG_DWORD, bytes.as_ptr(), 4) };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(win32("set registry DWORD", status))
    }
}

fn set_registry_string(path: &str, name: &str, value: &str) -> Result<()> {
    let key = open_registry(path, true, true)?;
    let words = wide(value);
    let status = unsafe {
        RegSetValueExW(
            key.0,
            wide(name).as_ptr(),
            0,
            REG_SZ,
            words.as_ptr().cast(),
            u32::try_from(words.len() * 2).expect("bounded"),
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(win32("set registry string", status))
    }
}

fn delete_registry_value(path: &str, name: &str) -> Result<()> {
    let key = match open_registry(path, true, false) {
        Ok(key) => key,
        Err(TuningError::Native { code, .. }) if code == i64::from(ERROR_FILE_NOT_FOUND) => {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let status = unsafe { RegDeleteValueW(key.0, wide(name).as_ptr()) };
    if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(win32("delete registry value", status))
    }
}

fn active_power_plan() -> Result<String> {
    let mut pointer = std::ptr::null_mut();
    let status = unsafe { PowerGetActiveScheme(std::ptr::null_mut(), &mut pointer) };
    if status != ERROR_SUCCESS {
        return Err(win32("read active power plan", status));
    }
    let guid = unsafe { *pointer };
    unsafe { LocalFree(pointer.cast()) };
    Ok(format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4[0],
        guid.data4[1],
        guid.data4[2],
        guid.data4[3],
        guid.data4[4],
        guid.data4[5],
        guid.data4[6],
        guid.data4[7]
    ))
}

fn set_power_plan(value: &str) -> Result<()> {
    let id = uuid::Uuid::parse_str(
        value.trim_matches(|c: char| c.is_whitespace() || c == '{' || c == '}'),
    )
    .map_err(|_| TuningError::BackupInvalid("power plan GUID is invalid".into()))?;
    let (data1, data2, data3, data4) = id.as_fields();
    let guid = GUID {
        data1,
        data2,
        data3,
        data4: *data4,
    };
    let status = unsafe { PowerSetActiveScheme(std::ptr::null_mut(), &guid) };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(win32("set active power plan", status))
    }
}

pub(super) fn staging_parent() -> Result<PathBuf> {
    let base = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    validate_backup_parent(&base)?;
    let privileged = base.join("NetworkLantern-Privileged");
    initialize_protected_directory(&privileged)?;
    let staging = privileged.join("RestoreStaging");
    initialize_protected_directory(&staging)?;
    Ok(staging)
}

pub(super) fn validate_backup_acl(path: &Path) -> Result<()> {
    validate_protected(path)
}
pub(super) fn validate_backup_parent(path: &Path) -> Result<()> {
    let mut cursor = Some(path);
    while let Some(ancestor) = cursor {
        if ancestor.exists() {
            reject_reparse(ancestor)?;
            let sddl = path_sddl(
                ancestor,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            )?;
            if !sddl.starts_with("O:SY") && !sddl.starts_with("O:BA") && !sddl.starts_with("O:TI") {
                return Err(TuningError::BackupInvalid(format!(
                    "backup ancestor is not owned by SYSTEM, Administrators, or TrustedInstaller: {}",
                    ancestor.display()
                )));
            }
            if grants_untrusted_write(&sddl) {
                return Err(TuningError::BackupInvalid(format!(
                    "backup ancestor grants write access to an untrusted broad principal: {}",
                    ancestor.display()
                )));
            }
        }
        cursor = ancestor.parent();
    }
    Ok(())
}
pub(super) fn validate_staging_parent(path: &Path) -> Result<()> {
    validate_staging_chain(path)
}
pub(super) fn validate_staging_path(path: &Path) -> Result<()> {
    validate_staging_chain(path)
}

fn initialize_protected_directory(path: &Path) -> Result<()> {
    match fs::create_dir(path) {
        Ok(()) => protect_directory(path),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => validate_protected(path),
        Err(source) => Err(TuningError::io("create restore staging directory", source)),
    }
}

fn validate_staging_chain(path: &Path) -> Result<()> {
    let base = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    let privileged = base.join("NetworkLantern-Privileged");
    let root = privileged.join("RestoreStaging");
    if !path.starts_with(&root) {
        return Err(TuningError::BackupInvalid(
            "restore staging path moved outside its trusted root".into(),
        ));
    }
    validate_backup_parent(&base)?;
    validate_protected(&privileged)?;
    validate_protected(&root)?;
    let relative = path.strip_prefix(&root).expect("prefix checked");
    let mut current = root;
    for component in relative.components() {
        current.push(component.as_os_str());
        validate_protected(&current)?;
    }
    Ok(())
}
pub(super) fn protect_directory(path: &Path) -> Result<()> {
    protect(path)
}
pub(super) fn protect_file(path: &Path) -> Result<()> {
    protect(path)
}

fn protect(path: &Path) -> Result<()> {
    reject_reparse(path)?;
    let sddl = wide("O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)");
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(win32("create protected DACL", unsafe {
            windows_sys::Win32::Foundation::GetLastError()
        }));
    }
    let mut owner: PSID = std::ptr::null_mut();
    let mut group: PSID = std::ptr::null_mut();
    let mut owner_defaulted = 0;
    let mut group_defaulted = 0;
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl = std::ptr::null_mut();
    if unsafe { GetSecurityDescriptorOwner(descriptor, &mut owner, &mut owner_defaulted) } == 0
        || unsafe { GetSecurityDescriptorGroup(descriptor, &mut group, &mut group_defaulted) } == 0
    {
        unsafe { LocalFree(descriptor.cast()) };
        return Err(win32("read protected owner and group", unsafe {
            windows_sys::Win32::Foundation::GetLastError()
        }));
    }
    if unsafe {
        windows_sys::Win32::Security::GetSecurityDescriptorDacl(
            descriptor,
            &mut present,
            &mut dacl,
            &mut defaulted,
        )
    } == 0
        || present == 0
    {
        unsafe { LocalFree(descriptor.cast()) };
        return Err(win32("read protected DACL", unsafe {
            windows_sys::Win32::Foundation::GetLastError()
        }));
    }
    let status = unsafe {
        SetNamedSecurityInfoW(
            wide_os(path).as_mut_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION
                | GROUP_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION,
            owner,
            group,
            dacl,
            std::ptr::null(),
        )
    };
    unsafe { LocalFree(descriptor.cast()) };
    if status != ERROR_SUCCESS {
        return Err(win32("apply protected DACL", status));
    }
    validate_protected(path)
}

fn validate_protected(path: &Path) -> Result<()> {
    reject_reparse(path)?;
    let mut descriptor = std::ptr::null_mut();
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide_os(path).as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(win32("read protected path ACL", status));
    }
    let mut control = 0;
    let mut revision = 0;
    let ok = unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) };
    if ok == 0 || control & SE_DACL_PROTECTED == 0 {
        unsafe { LocalFree(descriptor.cast()) };
        return Err(TuningError::BackupInvalid(
            "path DACL is not protected".into(),
        ));
    }
    let sddl = descriptor_sddl(
        descriptor,
        OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
    );
    unsafe { LocalFree(descriptor.cast()) };
    let sddl = sddl?;
    let exact = [
        "O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)",
        "O:BAG:BAD:P(A;;FA;;;BA)(A;;FA;;;SY)",
    ];
    if !exact.contains(&sddl.as_str()) {
        return Err(TuningError::BackupInvalid(
            "path owner, group, or DACL differs from Administrators and SYSTEM full control".into(),
        ));
    }
    Ok(())
}

fn path_sddl(path: &Path, information: u32) -> Result<String> {
    let mut descriptor = std::ptr::null_mut();
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide_os(path).as_ptr(),
            SE_FILE_OBJECT,
            information,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(win32("read path security", status));
    }
    let result = descriptor_sddl(descriptor, information);
    unsafe { LocalFree(descriptor.cast()) };
    result
}

fn descriptor_sddl(descriptor: PSECURITY_DESCRIPTOR, information: u32) -> Result<String> {
    let mut string = std::ptr::null_mut();
    let mut length = 0;
    if unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            1,
            information,
            &mut string,
            &mut length,
        )
    } == 0
    {
        return Err(win32("encode path security descriptor", unsafe {
            windows_sys::Win32::Foundation::GetLastError()
        }));
    }
    let value =
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(string, length as usize) })
            .trim_end_matches('\0')
            .to_owned();
    unsafe { LocalFree(string.cast()) };
    Ok(value)
}

fn grants_untrusted_write(sddl: &str) -> bool {
    sddl.split('(')
        .skip(1)
        .filter_map(|ace| ace.split_once(')').map(|value| value.0))
        .any(|ace| {
            let fields = ace.split(';').collect::<Vec<_>>();
            if fields.len() < 6
                || fields[0] != "A"
                || !matches!(fields[5], "WD" | "AU" | "BU" | "AN")
            {
                return false;
            }
            let rights = fields[2];
            ["GA", "GW", "FA", "FW", "WD", "AD", "DC", "WO"]
                .iter()
                .any(|value| rights.contains(value))
                || rights
                    .strip_prefix("0x")
                    .and_then(|value| u32::from_str_radix(value, 16).ok())
                    .is_some_and(|mask| mask & 0x4000_0000 != 0 || mask & 0x000d_0116 != 0)
        })
}

fn reject_reparse(path: &Path) -> Result<()> {
    use std::os::windows::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| TuningError::io("inspect protected path", source))?;
    if metadata.file_attributes() & 0x400 != 0 || metadata.file_type().is_symlink() {
        return Err(TuningError::BackupInvalid(format!(
            "protected path is a reparse point: {}",
            path.display()
        )));
    }
    Ok(())
}

struct WindowsPublicationFs;

impl PublicationFs for WindowsPublicationFs {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn validate(&self, path: &Path) -> Result<FileIdentity> {
        let before = trusted_io::directory_identity(path)?;
        validate_backup_bundle(path)?;
        let after = trusted_io::directory_identity(path)?;
        if before != after {
            return Err(TuningError::BackupInvalid(format!(
                "backup directory was replaced during validation: {}",
                path.display()
            )));
        }
        Ok(after)
    }

    fn identity(&self, path: &Path) -> Result<FileIdentity> {
        trusted_io::directory_identity(path)
    }

    fn rename(&self, from: &Path, to: &Path, expected: FileIdentity) -> Result<()> {
        if trusted_io::directory_identity(from)? != expected {
            return Err(TuningError::BackupInvalid(format!(
                "backup directory was replaced before rename: {}",
                from.display()
            )));
        }
        let from = wide_os(from);
        let to = wide_os(to);
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(win32("rename backup directory", unsafe {
                windows_sys::Win32::Foundation::GetLastError()
            }));
        }
        Ok(())
    }

    fn remove_if_identity(&self, path: &Path, identity: FileIdentity) -> Result<()> {
        trusted_io::remove_owned_directory(path, identity)
    }

    fn sync_parent(&self, parent: &Path) -> Result<()> {
        TrustedDirectory::open(parent, "open backup parent for flush")?.sync()
    }
}

struct OwnedPublicationTree {
    path: PathBuf,
    identity: FileIdentity,
    armed: bool,
}

impl OwnedPublicationTree {
    fn new(path: PathBuf) -> Result<Self> {
        let identity = trusted_io::directory_identity(&path)?;
        Ok(Self {
            path,
            identity,
            armed: true,
        })
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for OwnedPublicationTree {
    fn drop(&mut self) {
        if self.armed {
            let _ = trusted_io::remove_owned_directory(&self.path, self.identity);
        }
    }
}

struct ProtectedBackupFile {
    path: PathBuf,
    file: Option<File>,
    identity: Option<FileIdentity>,
}

impl DurableFile for ProtectedBackupFile {
    fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        self.file
            .as_mut()
            .expect("backup file is writable")
            .write_all(bytes)
            .map_err(|source| TuningError::io("write backup artifact", source))
    }

    fn flush(&mut self) -> Result<()> {
        self.file
            .as_ref()
            .expect("backup file is writable")
            .sync_all()
            .map_err(|source| TuningError::io("flush backup artifact", source))
    }

    fn protect(&mut self) -> Result<()> {
        protect_file(&self.path)?;
        let identity = trusted_io::file_identity(
            self.file.as_ref().expect("backup file is writable"),
            "identify written backup file",
        )?;
        self.file.take();
        let file = trusted_io::open_retained_file(&self.path, "reopen protected backup file")?;
        if trusted_io::file_identity(&file, "identify protected backup file")? != identity {
            return Err(TuningError::BackupInvalid(
                "backup file was replaced while applying protection".into(),
            ));
        }
        self.identity = Some(identity);
        self.file = Some(file);
        Ok(())
    }

    fn read_back(&mut self, maximum: u64) -> Result<Vec<u8>> {
        let file = self.file.as_ref().expect("backup file is protected");
        if trusted_io::file_identity(file, "identify protected backup file")?
            != self.identity.expect("backup file identity is recorded")
            || !trusted_io::identity_matches_path(file, &self.path)?
        {
            return Err(TuningError::BackupInvalid(
                "backup file was replaced before verification".into(),
            ));
        }
        trusted_io::read_retained_bounded(file, maximum, "verify backup artifact bytes")
    }
}

fn write_protected(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|source| TuningError::io("create backup artifact", source))?;
    let mut file = ProtectedBackupFile {
        path: path.to_path_buf(),
        file: Some(file),
        identity: None,
    };
    persist::persist_exact(&mut file, bytes)
}

fn check_cancel(cancellation: &CancellationToken) -> Result<()> {
    if cancellation.is_cancelled() {
        Err(TuningError::Cancelled)
    } else {
        Ok(())
    }
}

fn win32(operation: &'static str, code: u32) -> TuningError {
    TuningError::Native {
        operation,
        code: i64::from(code),
        message: std::io::Error::from_raw_os_error(code as i32).to_string(),
    }
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
fn wide_os(value: &Path) -> Vec<u16> {
    value
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
