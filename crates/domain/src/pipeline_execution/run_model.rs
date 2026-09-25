use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineRun {
    pub id: Uuid,
    pub scope_id: Uuid,
    pub slice_id: Uuid,
    pub slice_revision: i64,
    pub revision: i64,
    pub definition_kind: PipelineKind,
    pub definition_version: String,
    pub definition_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_option_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_plan_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_plan_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_plan_digest: Option<String>,
    pub delivery_mode: PipelineDeliveryMode,
    pub qualification_reason: String,
    pub status: PipelineRunStatus,
    pub current_phase_id: Option<String>,
    pub current_phase_ordinal: Option<u32>,
}
