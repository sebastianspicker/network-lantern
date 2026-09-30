use crate::{Result, TuningError};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{MapAccess, Visitor},
};
use std::{
    collections::{BTreeMap, HashSet},
    fmt,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArtifactKind {
    SystemProfile,
    AfdParameters,
    QosPolicies,
    NicAdvanced,
    NicRsc,
    PowerPlan,
}

impl ArtifactKind {
    pub const ALL: [Self; 6] = [
        Self::SystemProfile,
        Self::AfdParameters,
        Self::QosPolicies,
        Self::NicAdvanced,
        Self::NicRsc,
        Self::PowerPlan,
    ];

    pub const fn file_name(self) -> &'static str {
        match self {
            Self::SystemProfile => "SystemProfile.reg",
            Self::AfdParameters => "AFD_Parameters.reg",
            Self::QosPolicies => "qos_ours.xml",
            Self::NicAdvanced => "nic_advanced_backup.csv",
            Self::NicRsc => "rsc_backup.csv",
            Self::PowerPlan => "powerplan.txt",
        }
    }

    pub const fn max_bytes(self) -> u64 {
        match self {
            Self::QosPolicies => super::MAX_QOS_BYTES,
            Self::PowerPlan => 64,
            Self::SystemProfile | Self::AfdParameters => super::MAX_REGISTRY_BYTES,
            Self::NicAdvanced | Self::NicRsc => super::MAX_CSV_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupManifestComponents {
    #[serde(rename = "SystemProfile")]
    pub system_profile: bool,
    #[serde(rename = "AfdParameters")]
    pub afd_parameters: bool,
    #[serde(rename = "QosPolicies")]
    pub qos_policies: bool,
    #[serde(rename = "NicAdvanced")]
    pub nic_advanced: bool,
    #[serde(rename = "NicRsc")]
    pub nic_rsc: bool,
    #[serde(rename = "PowerPlan")]
    pub power_plan: bool,
}

impl BackupManifestComponents {
    fn enabled(&self, kind: ArtifactKind) -> bool {
        match kind {
            ArtifactKind::SystemProfile => self.system_profile,
            ArtifactKind::AfdParameters => self.afd_parameters,
            ArtifactKind::QosPolicies => self.qos_policies,
            ArtifactKind::NicAdvanced => self.nic_advanced,
            ArtifactKind::NicRsc => self.nic_rsc,
            ArtifactKind::PowerPlan => self.power_plan,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupManifest {
    #[serde(rename = "SchemaVersion")]
    pub schema_version: u32,
    #[serde(rename = "ToolName")]
    pub tool_name: String,
    #[serde(
        rename = "MachineName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub machine_name: Option<String>,
    #[serde(rename = "Platform", default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(rename = "OsVersion", default, skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    #[serde(
        rename = "ModuleVersion",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub module_version: Option<String>,
    #[serde(rename = "Timestamp", default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(rename = "Components")]
    pub components: BackupManifestComponents,
    #[serde(rename = "ArtifactDigests")]
    #[serde(deserialize_with = "deserialize_digests")]
    pub artifact_digests: BTreeMap<String, String>,
}

fn deserialize_digests<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct DigestVisitor;

    impl<'de> Visitor<'de> for DigestVisitor {
        type Value = BTreeMap<String, String>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an artifact digest object with unique names")
        }

        fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut values = BTreeMap::new();
            let mut folded = HashSet::new();
            while let Some((name, digest)) = map.next_entry::<String, String>()? {
                if !folded.insert(name.to_ascii_lowercase()) {
                    return Err(serde::de::Error::custom(format!(
                        "duplicate artifact digest or case variant: {name}"
                    )));
                }
                values.insert(name, digest);
            }
            Ok(values)
        }
    }

    deserializer.deserialize_map(DigestVisitor)
}

impl BackupManifest {
    pub fn expected_artifacts(&self) -> Vec<ArtifactKind> {
        ArtifactKind::ALL
            .into_iter()
            .filter(|kind| self.components.enabled(*kind))
            .collect()
    }
}

pub fn parse_manifest_bounded(bytes: &[u8]) -> Result<BackupManifest> {
    if bytes.len() as u64 > super::MAX_MANIFEST_BYTES {
        return Err(TuningError::BackupInvalid("manifest exceeds 1 MiB".into()));
    }
    enforce_json_depth(bytes, 32)?;
    let manifest: BackupManifest = serde_json::from_slice(bytes)
        .map_err(|error| TuningError::BackupInvalid(format!("invalid manifest JSON: {error}")))?;
    if !(1..=3).contains(&manifest.schema_version) {
        return Err(TuningError::BackupInvalid(
            "SchemaVersion must be 1, 2, or 3".into(),
        ));
    }
    if manifest.tool_name.trim().is_empty() {
        return Err(TuningError::BackupInvalid(
            "ToolName must not be empty".into(),
        ));
    }
    if manifest.expected_artifacts().is_empty() {
        return Err(TuningError::BackupInvalid(
            "at least one component must be enabled".into(),
        ));
    }
    let known: HashSet<&str> = ArtifactKind::ALL
        .into_iter()
        .map(ArtifactKind::file_name)
        .collect();
    let mut folded = HashSet::new();
    for (name, digest) in &manifest.artifact_digests {
        if !known.contains(name.as_str()) {
            return Err(TuningError::BackupInvalid(format!(
                "unknown artifact digest: {name}"
            )));
        }
        if !folded.insert(name.to_ascii_lowercase()) {
            return Err(TuningError::BackupInvalid(format!(
                "duplicate artifact digest: {name}"
            )));
        }
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
        {
            return Err(TuningError::BackupInvalid(format!(
                "invalid uppercase SHA-256 digest for {name}"
            )));
        }
    }
    Ok(manifest)
}

fn enforce_json_depth(bytes: &[u8], maximum: usize) -> Result<()> {
    let mut depth = 0usize;
    let mut string = false;
    let mut escaped = false;
    for byte in bytes {
        if string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                string = false;
            }
            continue;
        }
        match *byte {
            b'"' => string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > maximum {
                    return Err(TuningError::BackupInvalid(format!(
                        "manifest exceeds JSON depth {maximum}"
                    )));
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPONENTS: &str = r#""Components":{"SystemProfile":false,"AfdParameters":false,"QosPolicies":false,"NicAdvanced":false,"NicRsc":false,"PowerPlan":true}"#;

    #[test]
    fn rejects_duplicate_digest_case_variants() {
        let json = format!(
            r#"{{"SchemaVersion":3,"ToolName":"network-lantern",{COMPONENTS},"ArtifactDigests":{{"powerplan.txt":"{}","POWERPLAN.TXT":"{}"}}}}"#,
            "A".repeat(64),
            "B".repeat(64)
        );
        assert!(matches!(
            parse_manifest_bounded(json.as_bytes()),
            Err(TuningError::BackupInvalid(_))
        ));
    }

    #[test]
    fn accepts_all_supported_schema_versions() {
        for schema in 1..=3 {
            let json = format!(
                r#"{{"SchemaVersion":{schema},"ToolName":"network-lantern",{COMPONENTS},"ArtifactDigests":{{"powerplan.txt":"{}"}}}}"#,
                "A".repeat(64)
            );
            assert_eq!(
                parse_manifest_bounded(json.as_bytes())
                    .unwrap()
                    .schema_version,
                schema
            );
        }
    }

    #[test]
    fn schemas_one_to_three_keep_legacy_optional_metadata_optional() {
        for schema in 1..=3 {
            let json = format!(
                r#"{{"SchemaVersion":{schema},"ToolName":"network-diagnostics-suite",{COMPONENTS},"ArtifactDigests":{{"powerplan.txt":"{}"}}}}"#,
                "A".repeat(64)
            );
            assert!(parse_manifest_bounded(json.as_bytes()).is_ok());
        }
    }

    #[test]
    fn every_supported_schema_rejects_unknown_and_duplicate_metadata() {
        for schema in 1..=3 {
            let unknown = format!(
                r#"{{"SchemaVersion":{schema},"ToolName":"network-lantern",{COMPONENTS},"Unknown":true,"ArtifactDigests":{{"powerplan.txt":"{}"}}}}"#,
                "A".repeat(64)
            );
            assert!(parse_manifest_bounded(unknown.as_bytes()).is_err());
            let duplicate = format!(
                r#"{{"SchemaVersion":{schema},"SchemaVersion":{schema},"ToolName":"network-lantern",{COMPONENTS},"ArtifactDigests":{{"powerplan.txt":"{}"}}}}"#,
                "A".repeat(64)
            );
            assert!(parse_manifest_bounded(duplicate.as_bytes()).is_err());
        }
    }
}
