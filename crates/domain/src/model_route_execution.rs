//! Snapshot selection data, not a credential or permission to invoke a host.
//! Only the application-created current result may cross a trusted source port.
use crate::{
    CapturedModelRouteDecision, CapturedModelRouteDisposition, Error,
    PreparedModelRouteRecommendation, Result,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const MODEL_ROUTE_HOST_SELECTION_SCHEMA: &str = "tect.model-route-host-selection/1";
pub const MODEL_ROUTE_OWNED_STDIO_HOST: &str = "codex_app_server_owned_stdio";

/// Exact persisted locator, including the saved Work revision when applicable.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "level", rename_all = "snake_case")]
pub enum ModelRouteSourceLocator {
    Program {
        program_id: Uuid,
    },
    Scope {
        program_id: Uuid,
        scope_id: Uuid,
    },
    Slice {
        program_id: Uuid,
        scope_id: Uuid,
        candidate_set_id: Uuid,
        work_candidate_id: Uuid,
        expected_work_revision: i64,
    },
    OpenedSlice {
        slice_id: Uuid,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModelRouteExecutionSourceBinding {
    pub locator: ModelRouteSourceLocator,
    pub source_request_id: Uuid,
    pub source_recorded_by_actor_id: Uuid,
    pub source_recorded_by_session_id: Uuid,
    pub frozen_snapshot_id: Uuid,
    pub authority_schema: String,
    pub requirements_semantic_digest: String,
    pub matrix_save_actor_id: Uuid,
    pub matrix_save_session_id: Uuid,
    pub scope_id: Uuid,
    pub result_revision: i64,
    pub matrix_evaluation_digest: String,
    pub matrix_catalogue_version: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModelRouteExecutionDimension {
    pub route_id: String,
    pub provider: String,
    pub model: String,
    pub effort: String,
}

/// Immutable full snapshot. Serialization/deserialization is not authentication.
/// There is deliberately no Deserialize implementation or observed-actual claim.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModelRouteHostSelectionMaterial {
    pub schema: String,
    pub intended_host_kind: String,
    pub workspace_id: Uuid,
    pub invoking_actor_id: Uuid,
    pub invoking_session_id: Uuid,
    pub preparation: PreparedModelRouteRecommendation,
    pub decision: CapturedModelRouteDecision,
    pub disposition: CapturedModelRouteDisposition,
    pub source_binding: ModelRouteExecutionSourceBinding,
    pub requested_route: Option<ModelRouteExecutionDimension>,
    pub recommended_route: Option<ModelRouteExecutionDimension>,
    pub selected_route: ModelRouteExecutionDimension,
    pub configured_route: ModelRouteExecutionDimension,
    pub input_sha256: String,
    pub invocation_key: String,
}

impl ModelRouteHostSelectionMaterial {
    /// Compact UTF-8 JSON with recursively sorted object keys. Arrays keep their
    /// persisted order. Hash the entire returned bytes, with no digest field.
    pub fn canonical_json(&self) -> Result<String> {
        canonical_selection_json(self)
    }

    pub fn canonical_digest(&self) -> Result<String> {
        Ok(format!(
            "{:x}",
            Sha256::digest(self.canonical_json()?.as_bytes())
        ))
    }
}

fn canonical_selection_json<T: Serialize>(value: &T) -> Result<String> {
    fn ordered(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(values) => {
                let sorted: std::collections::BTreeMap<_, _> = values.into_iter().collect();
                serde_json::Value::Object(
                    sorted
                        .into_iter()
                        .map(|(key, value)| (key, ordered(value)))
                        .collect(),
                )
            }
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.into_iter().map(ordered).collect())
            }
            other => other,
        }
    }
    let value = serde_json::to_value(value).map_err(|_| Error::InvalidArguments)?;
    serde_json::to_string(&ordered(value)).map_err(|_| Error::InvalidArguments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_encoding_sorts_nested_keys_preserves_arrays_and_unicode() {
        let value = serde_json::json!({"z":["β", "a"], "a":{"y":2,"x":"тест"}});
        let expected = "{\"a\":{\"x\":\"тест\",\"y\":2},\"z\":[\"β\",\"a\"]}";
        assert_eq!(canonical_selection_json(&value).unwrap(), expected);
        assert_ne!(
            canonical_selection_json(&serde_json::json!({"z":["a", "β"],"a":{"y":2,"x":"тест"}}))
                .unwrap(),
            expected
        );
    }
}
