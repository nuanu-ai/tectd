use super::*;
use tect_domain::{SelectedSaveObservation, SelectedSaveObservationStatus};

#[test]
fn verifier_qualification_never_grants_approval_or_acceptance() {
    for status in [
        SelectedSaveObservationStatus::Passed,
        SelectedSaveObservationStatus::Failed,
    ] {
        for qualification in ["unresolved", "independently_observed"] {
            let id = uuid::Uuid::new_v4();
            let observation = SelectedSaveObservation {
                id,
                request_id: id,
                opportunity_id: id,
                candidate_set_id: id,
                caller_link_id: id,
                caller_receipt_request_id: id,
                target_revision: 1,
                actor_id: id,
                session_id: id,
                status,
                reason_codes: Vec::new(),
                evidence_digest: "a".repeat(64),
                qualification: qualification.into(),
            };
            let value = selected_save_response(observation);
            assert_eq!(value.as_object().unwrap().len(), 3);
            assert_eq!(value["establishes_independent_approval"], false);
            assert_eq!(value["establishes_current_acceptance"], false);
            assert_eq!(value["observation"]["qualification"], qualification);
            assert!(value.get("approval_granted").is_none());
            assert!(value.get("dev_acceptance_proven").is_none());
        }
    }
}
