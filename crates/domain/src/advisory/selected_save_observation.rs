use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedSaveObservationRequest {
    pub request_id: Uuid,
    pub opportunity_id: Uuid,
    pub candidate_set_id: Uuid,
    pub caller_link_id: Uuid,
    pub caller_receipt_request_id: Uuid,
    pub target_revision: i64,
    pub session_id: Uuid,
}

impl SelectedSaveObservationRequest {
    pub fn valid(&self) -> bool {
        !self.request_id.is_nil()
            && !self.opportunity_id.is_nil()
            && !self.candidate_set_id.is_nil()
            && !self.caller_link_id.is_nil()
            && !self.caller_receipt_request_id.is_nil()
            && !self.session_id.is_nil()
            && self.target_revision >= 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectedSaveObservationStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedSaveObservation {
    pub id: Uuid,
    pub request_id: Uuid,
    pub opportunity_id: Uuid,
    pub candidate_set_id: Uuid,
    pub caller_link_id: Uuid,
    pub caller_receipt_request_id: Uuid,
    pub target_revision: i64,
    pub actor_id: Uuid,
    pub session_id: Uuid,
    pub status: SelectedSaveObservationStatus,
    pub reason_codes: Vec<String>,
    pub evidence_digest: String,
    /// Qualification and independent verifier identity remain unresolved.
    pub qualification: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedSaveChecks {
    pub manifest: bool,
    pub advice: bool,
    pub disposition: bool,
    pub preservation: bool,
    pub caller: bool,
    pub receipt: bool,
    pub material: bool,
    pub revision: bool,
}

pub fn evaluate_selected_save_checks(
    checks: SelectedSaveChecks,
) -> (SelectedSaveObservationStatus, Vec<String>) {
    let reasons = [
        (checks.manifest, "manifest_missing_or_invalid"),
        (checks.advice, "advice_missing_or_invalid"),
        (checks.disposition, "disposition_missing_or_mismatched"),
        (checks.preservation, "preservation_missing_or_failed"),
        (checks.caller, "caller_link_missing_or_mismatched"),
        (checks.receipt, "candidate_receipt_missing_or_mismatched"),
        (checks.material, "saved_material_missing_or_mismatched"),
        (checks.revision, "candidate_revision_stale"),
    ]
    .into_iter()
    .filter_map(|(passed, reason)| (!passed).then(|| reason.to_owned()))
    .collect::<Vec<_>>();
    let status = if reasons.is_empty() {
        SelectedSaveObservationStatus::Passed
    } else {
        SelectedSaveObservationStatus::Failed
    };
    (status, reasons)
}
