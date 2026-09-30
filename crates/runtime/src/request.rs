//! Request envelope parsing and the capabilities accepted at the request boundary.
use crate::workflow::Workflow;
use lantern_contracts::{Error, Result};
use lantern_platform::PROFILE_LIMIT;
use serde::Deserialize;
use serde_json::Value;

/// Capabilities accepted at the request boundary; wire names are the serialized strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Capability {
    Throughput,
    PathBasic,
    PathTrace,
    Workflow,
    Tuning,
}
impl Capability {
    pub(crate) fn parse(name: &str) -> Result<Self> {
        match name {
            "throughput" => Ok(Self::Throughput),
            "path_basic" => Ok(Self::PathBasic),
            "path_trace" => Ok(Self::PathTrace),
            "workflow" => Ok(Self::Workflow),
            "tuning" => Ok(Self::Tuning),
            _ => Err(Error::validation("Unknown capability")),
        }
    }
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Throughput => "throughput",
            Self::PathBasic => "path_basic",
            Self::PathTrace => "path_trace",
            Self::Workflow => "workflow",
            Self::Tuning => "tuning",
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRequest {
    capability: String,
    #[serde(default)]
    layers: Vec<Value>,
    #[serde(default)]
    strict: bool,
    workflow: Option<Workflow>,
}
pub(crate) struct Request {
    pub capability: Capability,
    pub layers: Vec<Value>,
    pub strict: bool,
    pub workflow: Option<Workflow>,
}
pub(crate) fn request(value: &Value) -> Result<Request> {
    if serde_json::to_vec(value)
        .map_err(|e| Error::validation(e.to_string()))?
        .len()
        > PROFILE_LIMIT
    {
        return Err(Error::validation("Request exceeds 1 MiB"));
    }
    let mut request: RawRequest =
        serde_json::from_value(value.clone()).map_err(|e| Error::validation(e.to_string()))?;
    for layer in &mut request.layers {
        if layer.get("schema_version").is_some() || layer.get("capability").is_some() {
            if layer.get("schema_version").and_then(Value::as_u64) != Some(1)
                || !layer.get("parameters").is_some_and(Value::is_object)
                || layer.as_object().unwrap().keys().any(|k| {
                    !["schema_version", "capability", "parameters", "workflow"]
                        .contains(&k.as_str())
                })
            {
                return Err(Error::validation("Invalid or unsupported profile envelope"));
            }
            if layer["capability"] != request.capability {
                return Err(Error::validation(
                    "Profile capability does not match requested capability",
                ));
            }
            if request.capability == "workflow"
                && let Some(flow) = layer.get("workflow")
            {
                let saved: Workflow = serde_json::from_value(flow.clone())
                    .map_err(|e| Error::validation(e.to_string()))?;
                if request.workflow.is_some_and(|selected| selected != saved) {
                    return Err(Error::validation(
                        "Saved workflow does not match the selected workflow",
                    ));
                }
                request.workflow = Some(saved);
            }
            *layer = layer
                .get("parameters")
                .cloned()
                .ok_or_else(|| Error::validation("Profile parameters missing"))?;
        }
    }
    Ok(Request {
        capability: Capability::parse(&request.capability)?,
        layers: request.layers,
        strict: request.strict,
        workflow: request.workflow,
    })
}
