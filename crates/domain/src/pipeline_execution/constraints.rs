use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PipelineOutputConstraint {
    EngineeringReview {
        stage: String,
        standards_resource_id: String,
        standards_resource_digest: String,
        artifact_name: String,
        success_verdicts: Vec<String>,
        #[serde(default)]
        required_prior_review_phase_ids: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        required_reconciliation_phase_id: Option<String>,
    },
    CodeAuthorization {
        required_plan_review_phase_id: String,
    },
    ResolvedKnowledgePublication {
        when_verdicts: Vec<String>,
    },
    ReviewerContextMode {
        field: String,
        independent_value: String,
        self_value: String,
    },
    FieldRequired {
        field: String,
        #[serde(default)]
        when_verdict: Option<String>,
    },
    FieldEquals {
        field: String,
        value: String,
        #[serde(default)]
        when_verdict: Option<String>,
    },
    FieldNotEquals {
        field: String,
        value: String,
        #[serde(default)]
        when_verdict: Option<String>,
    },
    FieldIntegerEquals {
        field: String,
        value: i64,
        #[serde(default)]
        when_verdict: Option<String>,
    },
    FieldIntegerNotEquals {
        field: String,
        value: i64,
        #[serde(default)]
        when_verdict: Option<String>,
    },
    FieldIntegerMinimum {
        field: String,
        value: i64,
        #[serde(default)]
        when_verdict: Option<String>,
    },
    FieldBooleanEquals {
        field: String,
        value: bool,
        #[serde(default)]
        when_verdict: Option<String>,
    },
    FieldOneOf {
        field: String,
        values: Vec<String>,
        #[serde(default)]
        when_verdict: Option<String>,
    },
    FieldsEqual {
        field: String,
        other_field: String,
        #[serde(default)]
        when_verdict: Option<String>,
    },
    CommandReceipt {
        field: String,
        required_status: String,
        required_scope: String,
        require_nonzero_exit: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_field: Option<String>,
        #[serde(default)]
        when_verdict: Option<String>,
    },
}
