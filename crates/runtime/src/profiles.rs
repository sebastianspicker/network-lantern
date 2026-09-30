use lantern_contracts::{Error, Result, now};
use lantern_platform::{PROFILE_LIMIT, SidecarLock, atomic_json, read_json};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct ProfileStore {
    pub path: PathBuf,
}
impl Default for ProfileStore {
    fn default() -> Self {
        Self {
            path: PathBuf::from(".iperf3/profiles.json"),
        }
    }
}
impl ProfileStore {
    pub fn read(&self) -> Result<Value> {
        self.validate_provenance()?;
        if !self.path.try_exists()? {
            return Ok(json!({"version":2,"profiles":{}}));
        }
        let value = read_json(&self.path, PROFILE_LIMIT)?;
        if !value.get("profiles").is_some_and(Value::is_object) {
            return Err(Error::validation(
                "Profile store must contain a profiles object",
            ));
        }
        Ok(value)
    }
    pub fn list(&self) -> Result<Vec<String>> {
        Ok(self.read()?["profiles"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect())
    }
    pub fn get(&self, name: &str) -> Result<Value> {
        validate_name(name)?;
        self.read()?["profiles"]
            .get(name)
            .cloned()
            .ok_or_else(|| Error::validation(format!("Profile '{name}' does not exist")))
    }
    pub fn save(&self, name: &str, parameters: &Value) -> Result<()> {
        validate_name(name)?;
        validate_parameters(parameters)?;
        if !parameters.is_object() {
            return Err(Error::validation("Profile must be a JSON object"));
        }
        let _lock = self.lock()?;
        let mut store = self.read()?;
        store["profiles"][name] = parameters.clone();
        store["version"] = json!(2);
        store["updatedUtc"] = json!(now());
        atomic_json(&self.path, &store, PROFILE_LIMIT)
    }
    pub fn delete(&self, name: &str) -> Result<bool> {
        validate_name(name)?;
        let _lock = self.lock()?;
        let mut store = self.read()?;
        let removed = store["profiles"]
            .as_object_mut()
            .unwrap()
            .remove(name)
            .is_some();
        if removed {
            store["updatedUtc"] = json!(now());
            atomic_json(&self.path, &store, PROFILE_LIMIT)?;
        }
        Ok(removed)
    }
    fn validate_provenance(&self) -> Result<()> {
        if !self.path.is_absolute() && lantern_platform::is_elevated()? {
            return Err(Error::validation(
                "Elevated profile access requires an explicit absolute path",
            ));
        }
        Ok(())
    }
    fn lock(&self) -> Result<SidecarLock> {
        self.validate_provenance()?;
        let mut name = self.path.as_os_str().to_os_string();
        name.push(".lock");
        SidecarLock::acquire(Path::new(&name), Duration::from_secs(15))
    }
}
pub fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty()
        || name.chars().count() > 128
        || name
            .chars()
            .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
    {
        return Err(Error::validation(
            "Profile name must be 1–128 characters without control or path characters",
        ));
    }
    Ok(())
}

/// Validate stored control input without requiring a target in a partial throughput profile.
pub fn validate_parameters(parameters: &Value) -> Result<()> {
    if serde_json::to_vec(parameters)
        .map_err(|e| Error::validation(e.to_string()))?
        .len()
        > PROFILE_LIMIT
    {
        return Err(Error::validation("Profile exceeds 1 MiB"));
    }
    if !parameters.is_object() {
        return Err(Error::validation("Profile must be a JSON object"));
    }
    if parameters.get("schema_version").is_some() || parameters.get("capability").is_some() {
        crate::plan_request(
            &json!({"capability":parameters["capability"],"layers":[parameters],"strict":true}),
        )?;
    } else if ["path", "throughput", "windowsTuning"]
        .iter()
        .any(|key| parameters.get(key).is_some())
    {
        crate::workflow::plan(
            crate::workflow::Workflow::Triage,
            parameters,
            &json!({}),
            true,
        )?;
    } else {
        let mut candidate = parameters.clone();
        if !candidate
            .as_object()
            .unwrap()
            .keys()
            .any(|key| key.eq_ignore_ascii_case("target"))
        {
            candidate["target"] = json!("profile-validation.invalid");
        }
        // A profile can intentionally constrain a future plan below its default size.
        for (key, value) in candidate.as_object().unwrap() {
            if (key.eq_ignore_ascii_case("max_total_tests")
                || key.eq_ignore_ascii_case("MaxTotalTests"))
                && !value.is_null()
                && value.as_u64().is_none_or(|n| n > 1_000_000)
            {
                return Err(Error::validation(
                    "max_total_tests must be a nonnegative integer or null",
                ));
            }
        }
        candidate["max_total_tests"] = json!(0);
        crate::plan_request(
            &json!({"capability":"throughput","layers":[candidate],"strict":true}),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_store_preserves_unknown_fields_and_other_profiles() {
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let path = dir.path().join("profiles.json");
        atomic_json(&path, &json!({"version":1,"extra":true,"profiles":{"old":{"Target":"example.invalid","MaxTotalTests":0}}}),PROFILE_LIMIT).unwrap();
        let store = ProfileStore { path };
        assert_eq!(store.get("old").unwrap()["MaxTotalTests"], 0);
        store.save("new", &json!({"target":"localhost"})).unwrap();
        assert!(store.read().unwrap()["extra"].as_bool().unwrap());
        assert_eq!(store.list().unwrap(), ["new", "old"]);
        assert!(store.delete("new").unwrap());
        assert!(!store.delete("new").unwrap());
    }
    #[test]
    fn invalid_budget_and_oversize_profile_leave_store_untouched() {
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let store = ProfileStore {
            path: dir.path().join("profiles.json"),
        };
        store
            .save("existing", &json!({"target":"fixture.invalid"}))
            .unwrap();
        let before = std::fs::read(&store.path).unwrap();
        for invalid in [
            json!({"max_total_tests":-1}),
            json!({"MaxTotalTests":"many"}),
            json!({"target":"x".repeat(PROFILE_LIMIT+1)}),
        ] {
            assert!(store.save("invalid", &invalid).is_err());
            assert_eq!(std::fs::read(&store.path).unwrap(), before);
        }
    }
    #[test]
    fn invalid_store_never_replaced() {
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let path = dir.path().join("profiles.json");
        std::fs::write(&path, b"broken").unwrap();
        assert!(
            ProfileStore { path: path.clone() }
                .save("valid", &json!({}))
                .is_err()
        );
        assert_eq!(std::fs::read(path).unwrap(), b"broken");
    }
}
