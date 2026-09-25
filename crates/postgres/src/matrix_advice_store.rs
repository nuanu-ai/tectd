use async_trait::async_trait;
use sha2::{Digest, Sha256};
use sqlx::{Row, postgres::PgRow};
use tect_application::{
    AdvisoryLifecycleCapability, GuardedMatrixAdviceOutcome, GuardedMatrixAdviceRecord,
    MatrixAdviceStore, MatrixProviderBinding, MatrixTaskStore, StoredGuardedMatrixAdviceRecord,
    canonical_matrix_advice_digest, canonical_matrix_input_digest,
};
use tect_domain::{
    AdvisoryDispatch, AdvisoryModelConfiguration, AdvisoryOpportunity, AdvisoryOpportunityState,
    AdvisoryProviderProfileRef, EngineeringChoiceSet, EngineeringMatrixInput, Error,
    MatrixVerificationRecord, OwnerReportedEngineeringMatrixFacts, Result,
    ValidatedMatrixVerification, compose_independently_verified_owner_matrix,
    evaluate_matrix_verification, matrix_verified_evaluation_digest,
};
use uuid::Uuid;

use crate::{advisory::finalize_opportunity, storage_error, store::PgUnitOfWork};

fn decode_advice(
    row: PgRow,
    binding: MatrixProviderBinding,
    raw: Vec<u8>,
) -> Result<StoredGuardedMatrixAdviceRecord> {
    let kind: String = row.try_get("kind").map_err(storage_error)?;
    let ranks: Option<serde_json::Value> =
        row.try_get("ranked_choice_ids").map_err(storage_error)?;
    let reason: Option<String> = row.try_get("reason").map_err(storage_error)?;
    let outcome = match (kind.as_str(), ranks, reason) {
        ("ranked", Some(ranks), None) => GuardedMatrixAdviceOutcome::Ranked {
            ranked_choice_ids: serde_json::from_value(ranks)
                .map_err(|_| Error::InternalInvariant)?,
        },
        ("abstained", None, reason) => GuardedMatrixAdviceOutcome::Abstained { reason },
        ("rejected", None, Some(reason)) => GuardedMatrixAdviceOutcome::Rejected { reason },
        _ => return Err(Error::InternalInvariant),
    };
    let profile: String = row.try_get("provider_profile_ref").map_err(storage_error)?;
    let model: serde_json::Value = row.try_get("model_configuration").map_err(storage_error)?;
    let model_configuration: AdvisoryModelConfiguration =
        serde_json::from_value(model).map_err(|_| Error::InternalInvariant)?;
    let record = GuardedMatrixAdviceRecord {
        opportunity_id: row.try_get("opportunity_id").map_err(storage_error)?,
        dispatch_id: row.try_get("dispatch_id").map_err(storage_error)?,
        opportunity_material_digest: binding.evaluation_digest.clone(),
        binding,
        provider_profile_ref: AdvisoryProviderProfileRef { id: profile },
        model_configuration,
        raw_response_payload: raw,
        response_payload_sha256: row
            .try_get("response_payload_sha256")
            .map_err(storage_error)?,
        advice_digest: row.try_get("advice_digest").map_err(storage_error)?,
        outcome,
    };
    if record.response_payload_sha256
        != format!("{:x}", Sha256::digest(&record.raw_response_payload))
        || record.advice_digest != canonical_matrix_advice_digest(&record.binding, &record.outcome)?
        || row.try_get::<Uuid, _>("task_id").map_err(storage_error)? != record.binding.task_id
        || row
            .try_get::<i64, _>("matrix_task_revision")
            .map_err(storage_error)?
            != record.binding.task_revision
        || row
            .try_get::<String, _>("matrix_choice_set_digest")
            .map_err(storage_error)?
            != record.binding.choice_set_digest
    {
        return Err(Error::InternalInvariant);
    }
    Ok(StoredGuardedMatrixAdviceRecord {
        advice_id: row.try_get("advice_id").map_err(storage_error)?,
        record,
    })
}

fn outcome_columns(
    outcome: &GuardedMatrixAdviceOutcome,
) -> (&'static str, Option<serde_json::Value>, Option<String>) {
    match outcome {
        GuardedMatrixAdviceOutcome::Ranked { ranked_choice_ids } => {
            ("ranked", Some(serde_json::json!(ranked_choice_ids)), None)
        }
        GuardedMatrixAdviceOutcome::Abstained { reason } => ("abstained", None, reason.clone()),
        GuardedMatrixAdviceOutcome::Rejected { reason } => ("rejected", None, Some(reason.clone())),
    }
}

fn validated_for_guarded_advice(
    task_id: Uuid,
    revision: i64,
    input: &EngineeringMatrixInput,
    verification: &MatrixVerificationRecord,
    verified_fresh_under_lock: bool,
    verified_epoch: i64,
    current_epoch: i64,
) -> Result<ValidatedMatrixVerification> {
    let validation_epoch = if verified_fresh_under_lock {
        verified_epoch
    } else {
        current_epoch
    };
    evaluate_matrix_verification(
        &task_id.to_string(),
        &revision.to_string(),
        input,
        verification,
        validation_epoch,
    )
    .map_err(|_| Error::StaleRevision)
}

mod implementation;

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::{
        EvidenceValidationOutcome, MATRIX_VERIFICATION_SCHEMA, MatrixEvidenceBinding,
        matrix_input_digest, required_matrix_facts,
    };

    #[test]
    fn rejected_outcome_has_no_ranking_or_usable_kind() {
        let (kind, ranks, reason) = outcome_columns(&GuardedMatrixAdviceOutcome::Rejected {
            reason: "guard rejected response".into(),
        });
        assert_eq!(kind, "rejected");
        assert!(ranks.is_none());
        assert_eq!(reason.as_deref(), Some("guard rejected response"));
    }

    #[test]
    fn finalize_freshness_survives_later_expiry_but_direct_persist_does_not() {
        let input: EngineeringMatrixInput = serde_json::from_value(serde_json::json!({
            "mode":{"state":"known","value":"demo","provenance":"owner"},
            "envelope":{"scale":{"state":"known","value":"one request","provenance":"owner"},
              "operational_facts":{"state":"known_empty","provenance":"owner"}},
            "criticality":{"state":"known","value":"low","provenance":"owner"},
            "intent":{"state":"known","value":{"kind":"other","description":"demo"},"provenance":"owner"},
            "urgency":{"state":"known","value":"ordinary","provenance":"owner"},
            "promised_behavior":{"state":"known","value":"demo","provenance":"owner"},
            "promised_proof":{"state":"known","value":"check","provenance":"owner"},
            "affected_guarantees":{"state":"known_empty","provenance":"owner"},
            "actual_exposure":{"state":"known","value":false,"provenance":"owner"},
            "demand_commitment":{"state":"known","value":"no_commitment","provenance":"owner"},
            "latency_commitment":{"state":"known","value":"no_commitment","provenance":"owner"},
            "urgent_repair":{"state":"known","value":false,"provenance":"owner"}
        })).unwrap();
        let task_id = Uuid::new_v4();
        let mut record = MatrixVerificationRecord {
            schema: MATRIX_VERIFICATION_SCHEMA.into(),
            task_id: task_id.to_string(),
            task_revision: "1".into(),
            input_digest: matrix_input_digest(&input).unwrap(),
            owner_principal: Uuid::new_v4().to_string(),
            verifier_principal: Uuid::new_v4().to_string(),
            policy_version: "test/1".into(),
            bindings: required_matrix_facts(&input)
                .unwrap()
                .into_iter()
                .map(|fact| MatrixEvidenceBinding {
                    fact_path: fact.path,
                    value_digest: fact.value_digest,
                    evidence_ref: "synthetic".into(),
                    content_digest: "a".repeat(64),
                    source: "synthetic".into(),
                    subject: "test".into(),
                    observed_at: 99,
                    expires_at: 101,
                    validation_outcome: EvidenceValidationOutcome::Accepted,
                })
                .collect(),
            digest: String::new(),
        };
        record.digest = record.canonical_digest().unwrap();
        assert!(validated_for_guarded_advice(task_id, 1, &input, &record, true, 100, 101).is_ok());
        assert_eq!(
            validated_for_guarded_advice(task_id, 1, &input, &record, false, 100, 101),
            Err(Error::StaleRevision)
        );
    }
}
