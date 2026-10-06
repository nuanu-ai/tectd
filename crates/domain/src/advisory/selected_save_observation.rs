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
    .filter(|(passed, _)| !*passed)
    .map(|(_, reason)| reason.to_owned())
    .collect::<Vec<_>>();
    let status = if reasons.is_empty() {
        SelectedSaveObservationStatus::Passed
    } else {
        SelectedSaveObservationStatus::Failed
    };
    (status, reasons)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_save_checks_preserve_exact_order_for_all_masks() {
        let expected_reasons = [
            "manifest_missing_or_invalid",
            "advice_missing_or_invalid",
            "disposition_missing_or_mismatched",
            "preservation_missing_or_failed",
            "caller_link_missing_or_mismatched",
            "candidate_receipt_missing_or_mismatched",
            "saved_material_missing_or_mismatched",
            "candidate_revision_stale",
        ];
        for mask in 0u16..256 {
            let checks = SelectedSaveChecks {
                manifest: mask & 1 != 0,
                advice: mask & (1 << 1) != 0,
                disposition: mask & (1 << 2) != 0,
                preservation: mask & (1 << 3) != 0,
                caller: mask & (1 << 4) != 0,
                receipt: mask & (1 << 5) != 0,
                material: mask & (1 << 6) != 0,
                revision: mask & (1 << 7) != 0,
            };
            let expected = expected_reasons
                .iter()
                .enumerate()
                .filter(|(bit, _)| mask & (1 << bit) == 0)
                .map(|(_, reason)| (*reason).to_owned())
                .collect::<Vec<_>>();
            let expected_status = if expected.is_empty() {
                SelectedSaveObservationStatus::Passed
            } else {
                SelectedSaveObservationStatus::Failed
            };
            let (status, reasons) = evaluate_selected_save_checks(checks);
            assert_eq!(reasons, expected, "mask {mask}");
            assert_eq!(status, expected_status, "mask {mask}");
        }
    }
}
