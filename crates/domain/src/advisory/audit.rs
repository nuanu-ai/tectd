use super::config::{
    AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryRequestPreference, WorkspaceAdvisoryConfig,
};
use super::lifecycle::{
    AdvisoryDispatchOutcome, AdvisoryDispatchState, AdvisoryOpportunityState, AdvisoryReason,
    AdvisoryRetryBasis, AdvisorySendCertainty,
};
use super::selected_save_observation::SelectedSaveObservationStatus;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdvisoryAuditQuery {
    pub limit: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<AdvisoryCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_point: Option<AdvisoryDecisionPoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<AdvisoryReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<AdvisoryOpportunityState>,
}
impl AdvisoryAuditQuery {
    pub fn validate(&self) -> Result<()> {
        if self.limit == 0
            || self.limit > 100
            || self.scope_id.is_some_and(|value| value.is_nil())
            || self.after.is_some_and(|value| value.is_nil())
            || self
                .decision_point
                .is_some_and(|point| self.capability.is_some_and(|value| !point.supports(value)))
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisoryAuditOpportunity {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub scope_id: Option<Uuid>,
    pub session_id: Uuid,
    pub authorized_actor_id: Uuid,
    pub work_item_kind: String,
    pub work_item_id: Option<Uuid>,
    pub source_revision: Option<String>,
    pub run_id: Option<Uuid>,
    pub phase: Option<String>,
    pub step: Option<String>,
    pub capability: AdvisoryCapability,
    pub decision_point: AdvisoryDecisionPoint,
    pub config_revision: i64,
    pub session_preference: AdvisoryRequestPreference,
    pub request_preference: AdvisoryRequestPreference,
    pub policy_version: String,
    pub request_key: String,
    pub material_digest: String,
    pub deterministic_baseline_ref: Option<String>,
    pub eligible_material_ref: Option<String>,
    pub state: AdvisoryOpportunityState,
    pub primary_reason: AdvisoryReason,
    pub parent_opportunity_id: Option<Uuid>,
    pub created_at: String,
    pub updated_at: String,
    pub guarded_advice_id: Option<Uuid>,
    pub guarded_advice_digest: Option<String>,
    pub disposition_id: Option<Uuid>,
    pub preservation_receipt_id: Option<Uuid>,
    pub preservation_status: Option<String>,
    pub caller_receipt_id: Option<Uuid>,
    pub caller_link_id: Option<Uuid>,
    pub verifier_receipt_id: Option<Uuid>,
    /// Latest server-computed observation at a recorded revision. A pass is
    /// neither independent approval nor acceptance of the current revision.
    pub selected_save_observation: Option<AdvisorySelectedSaveObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisorySelectedSaveObservation {
    pub id: Uuid,
    pub target_revision: i64,
    pub status: SelectedSaveObservationStatus,
    pub reason_codes: Vec<String>,
    pub evidence_digest: String,
    /// Independent qualification has not been resolved.
    pub qualification: String,
    pub establishes_independent_approval: bool,
    pub establishes_current_acceptance: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisoryAuditDispatch {
    pub id: Uuid,
    pub opportunity_id: Uuid,
    pub predecessor_dispatch_id: Option<Uuid>,
    pub attempt_number: i32,
    pub provider: String,
    pub model: String,
    pub configuration_digest: String,
    pub material_digest: String,
    pub payload_digest: String,
    pub request_bytes: i64,
    pub response_bytes: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub latency_ms: Option<i64>,
    pub state: AdvisoryDispatchState,
    pub send_certainty: AdvisorySendCertainty,
    pub outcome: Option<AdvisoryDispatchOutcome>,
    pub retry_basis: AdvisoryRetryBasis,
    pub raw_response_ref: Option<String>,
    pub authorized_at: String,
    pub send_started_at: Option<String>,
    pub sealed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisoryReasonCount {
    pub reason: AdvisoryReason,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AdvisoryAuditAggregate {
    pub opportunities: u64,
    pub opportunities_with_attempts: u64,
    pub no_call_opportunities: u64,
    pub no_call_by_reason: Vec<AdvisoryReasonCount>,
    pub authorized_attempts: u64,
    pub confirmed_sent_attempts: u64,
    pub send_unknown_attempts: u64,
    pub proven_unsent_attempts: u64,
    pub known_input_tokens: u64,
    pub known_output_tokens: u64,
    pub attempts_with_unknown_token_usage: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisoryAuditPage {
    pub config: WorkspaceAdvisoryConfig,
    pub opportunities: Vec<AdvisoryAuditOpportunity>,
    pub dispatches: Vec<AdvisoryAuditDispatch>,
    pub aggregate: AdvisoryAuditAggregate,
    pub next_after: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisoryOpportunityDetail {
    pub opportunity: AdvisoryAuditOpportunity,
    pub dispatches: Vec<AdvisoryAuditDispatch>,
}
