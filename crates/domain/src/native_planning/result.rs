use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SliceResultOutcome {
    Completed,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SliceResultEvidence {
    pub kind: String,
    pub reference: String,
    pub observation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeManagedResultProvenance {
    pub change_id: Uuid,
    pub run_id: Uuid,
    pub definition_version: String,
    pub definition_digest: String,
    pub final_attempt_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher_receipt_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher_receipt_digest: Option<String>,
    pub canonical: crate::KnowledgeCanonicalOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliceResult {
    pub id: Uuid,
    pub slice_id: Uuid,
    pub slice_revision: i64,
    pub revision: i64,
    pub outcome: SliceResultOutcome,
    pub summary: String,
    pub evidence: Vec<SliceResultEvidence>,
    pub scope_impact: String,
    pub remaining_work: String,
    pub provenance: String,
    pub pipeline_run_id: Option<Uuid>,
    pub pipeline_definition_version: Option<String>,
    pub pipeline_definition_digest: Option<String>,
    pub pipeline_final_attempt_id: Option<Uuid>,
    pub pipeline_result_origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knowledge_provenance: Option<KnowledgeManagedResultProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordSliceResultOutcome {
    Created {
        result: SliceResult,
        context: SliceCandidateContext,
    },
    Replay {
        result: SliceResult,
        context: SliceCandidateContext,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordSliceResult {
    pub request_id: Uuid,
    pub scope_id: Uuid,
    pub slice_id: Uuid,
    pub slice_revision: i64,
    pub outcome: SliceResultOutcome,
    pub summary: String,
    pub evidence: Vec<SliceResultEvidence>,
    pub scope_impact: String,
    pub remaining_work: String,
}
