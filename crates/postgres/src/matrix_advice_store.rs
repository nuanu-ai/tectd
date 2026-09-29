use async_trait::async_trait;
use sha2::{Digest, Sha256};
use sqlx::{Row, postgres::PgRow};
use tect_application::{
    AdvisoryLifecycleCapability, GuardedMatrixAdviceOutcome, GuardedMatrixAdviceRecord,
    MatrixAdviceStore, MatrixProviderBinding, MatrixRequirementsContextStore, MatrixTaskStore,
    MatrixVerificationAuthority, MatrixVerificationStore, StoredGuardedMatrixAdviceRecord,
    canonical_matrix_advice_digest, canonical_matrix_input_digest,
    canonical_matrix_trial_advice_digest, context_matrix_verified_evaluation_digest,
};
use tect_domain::{
    AdvisoryDispatch, AdvisoryModelConfiguration, AdvisoryOpportunity, AdvisoryOpportunityState,
    AdvisoryProviderProfileRef, CONTEXT_MATRIX_VERIFICATION_SCHEMA, EngineeringChoiceSet,
    EngineeringMatrixInput, Error, MATRIX_VERIFICATION_SCHEMA, MatrixTrialRankingEvidence,
    OwnerReportedEngineeringMatrixFacts, Result, compose_confirmed_requirements_matrix,
    compose_independently_verified_owner_matrix, evaluate_context_matrix_verification,
    evaluate_matrix_verification, matrix_verified_evaluation_digest,
};
use uuid::Uuid;

#[cfg(test)]
use tect_domain::{MatrixVerificationRecord, ValidatedMatrixVerification};

use crate::{advisory::finalize_matrix_response, storage_error, store::PgUnitOfWork};

pub(crate) fn verification_authority(row: &PgRow) -> Result<MatrixVerificationAuthority> {
    let digest: String = row.try_get("record_digest").map_err(storage_error)?;
    let schema: String = row.try_get("schema").map_err(storage_error)?;
    let snapshot: Option<Uuid> = row.try_get("frozen_snapshot_id").map_err(storage_error)?;
    let semantic: Option<String> = row
        .try_get("requirements_semantic_digest")
        .map_err(storage_error)?;
    let authority_schema: Option<String> =
        row.try_get("authority_schema").map_err(storage_error)?;
    match schema.as_str() {
        MATRIX_VERIFICATION_SCHEMA
            if snapshot.is_none() && semantic.is_none() && authority_schema.is_none() =>
        {
            Ok(MatrixVerificationAuthority::LegacyV1 { digest })
        }
        CONTEXT_MATRIX_VERIFICATION_SCHEMA => {
            let snapshot_id = snapshot.ok_or(Error::InternalInvariant)?;
            if snapshot_id.is_nil() {
                return Err(Error::InternalInvariant);
            }
            Ok(MatrixVerificationAuthority::ContextV2 {
                digest,
                snapshot_id,
                authority_schema: authority_schema.ok_or(Error::InternalInvariant)?,
                semantic_digest: semantic.ok_or(Error::InternalInvariant)?,
            })
        }
        _ => Err(Error::InternalInvariant),
    }
}

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
    let policy: Option<String> = row
        .try_get("ranking_policy_version")
        .map_err(storage_error)?;
    let trial_json: Option<serde_json::Value> =
        row.try_get("trial_uncertainty").map_err(storage_error)?;
    let signed_snapshot: serde_json::Value = row
        .try_get("configuration_snapshot")
        .map_err(storage_error)?;
    if policy.is_some() {
        let saved_digest: String = row.try_get("configuration_digest").map_err(storage_error)?;
        let actual_digest = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&signed_snapshot).map_err(|_| Error::InternalInvariant)?
            )
        );
        if actual_digest != saved_digest {
            return Err(Error::InternalInvariant);
        }
    }
    let trial_evidence =
        decode_trial_uncertainty(policy.as_deref(), trial_json, &signed_snapshot, &outcome)?;
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
        trial_evidence,
    };
    let expected_digest = match &record.trial_evidence {
        Some(evidence) => canonical_matrix_trial_advice_digest(
            &record.binding,
            &record.outcome,
            evidence,
            record.opportunity_id,
            record.dispatch_id,
            &record.response_payload_sha256,
        )
        .map_err(|_| Error::InternalInvariant)?,
        None => canonical_matrix_advice_digest(&record.binding, &record.outcome)
            .map_err(|_| Error::InternalInvariant)?,
    };
    if record.response_payload_sha256
        != format!("{:x}", Sha256::digest(&record.raw_response_payload))
        || record.advice_digest != expected_digest
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

fn decode_trial_uncertainty(
    policy: Option<&str>,
    trial_json: Option<serde_json::Value>,
    signed_snapshot: &serde_json::Value,
    outcome: &GuardedMatrixAdviceOutcome,
) -> Result<Option<MatrixTrialRankingEvidence>> {
    let signed_trial = match signed_snapshot.get("ranking_policy") {
        None => false, // Existing strict rows predate the explicit marker.
        Some(serde_json::Value::String(version))
            if version == tect_domain::MATRIX_NATIVE_RANKING_POLICY_VERSION =>
        {
            false
        }
        Some(serde_json::Value::String(version))
            if version == tect_domain::MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION =>
        {
            true
        }
        _ => return Err(Error::InternalInvariant),
    };
    if (policy.is_some() && !signed_trial)
        || (signed_trial
            && matches!(outcome, GuardedMatrixAdviceOutcome::Ranked { .. })
            && policy.is_none())
    {
        return Err(Error::InternalInvariant);
    }
    match (policy, trial_json, outcome) {
        (None, None, _) => Ok(None),
        (
            Some(tect_domain::MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION),
            Some(value),
            GuardedMatrixAdviceOutcome::Ranked { ranked_choice_ids },
        ) => {
            let evidence: MatrixTrialRankingEvidence =
                serde_json::from_value(value).map_err(|_| Error::InternalInvariant)?;
            evidence
                .validate_ranked(ranked_choice_ids)
                .map_err(|_| Error::InternalInvariant)?;
            Ok(Some(evidence))
        }
        _ => Err(Error::InternalInvariant),
    }
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

impl PgUnitOfWork {
    async fn insert_matrix_advice_receipt(
        &mut self,
        workspace_id: Uuid,
        record: &GuardedMatrixAdviceRecord,
    ) -> Result<StoredGuardedMatrixAdviceRecord> {
        let tenant = self.tenant_id()?;
        let (kind, ranks, reason) = outcome_columns(&record.outcome);
        let policy = record
            .trial_evidence
            .as_ref()
            .map(|evidence| evidence.policy_version.as_str());
        let uncertainty = record
            .trial_evidence
            .as_ref()
            .map(|evidence| serde_json::to_value(evidence).map_err(storage_error))
            .transpose()?;
        let inserted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO advisory_matrix_advice (tenant_id,workspace_id,opportunity_id,task_id,matrix_task_revision,matrix_choice_set_digest,dispatch_id,kind,ranked_choice_ids,reason,advice_digest,provider_profile_ref,model_configuration,response_payload_sha256,ranking_policy_version,trial_uncertainty) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16) ON CONFLICT DO NOTHING RETURNING advice_id"
        ).bind(tenant).bind(workspace_id).bind(record.opportunity_id).bind(record.binding.task_id)
            .bind(record.binding.task_revision).bind(&record.binding.choice_set_digest)
            .bind(record.dispatch_id).bind(kind).bind(ranks).bind(reason)
            .bind(&record.advice_digest).bind(&record.provider_profile_ref.id)
            .bind(serde_json::json!(record.model_configuration)).bind(&record.response_payload_sha256)
            .bind(policy).bind(uncertainty)
            .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        let advice_id = inserted.ok_or(Error::InputConflict)?;
        Ok(StoredGuardedMatrixAdviceRecord {
            advice_id,
            record: record.clone(),
        })
    }
}

#[cfg(test)]
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
    fn typed_trial_codec_requires_signed_policy_complete_metadata_and_valid_bounds() {
        use tect_domain::{
            MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION, MATRIX_TRIAL_POLICY_ID,
            MatrixTrialCandidateEvidence, NativeMatrixScoreDistribution,
            matrix_trial_policy_digest,
        };
        let score = |id: &str, level: usize, confidence: f64| {
            let mut probabilities = [0.0; 10];
            probabilities[level] = 1.0;
            let distribution = NativeMatrixScoreDistribution::new(probabilities).unwrap();
            let bounds = distribution.feasible_expected_score();
            MatrixTrialCandidateEvidence {
                candidate_id: id.into(),
                declared_score: level as f64,
                score_confidence: confidence,
                probabilities,
                displayed_mean: distribution.displayed_mean(),
                feasible_minimum: bounds.minimum,
                feasible_maximum: bounds.maximum,
            }
        };
        let evidence = MatrixTrialRankingEvidence {
            policy_id: MATRIX_TRIAL_POLICY_ID.into(),
            policy_version: MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION.into(),
            policy_digest: matrix_trial_policy_digest(),
            choice_selected_candidate_id: "a".into(),
            choice_confidence: 0.8,
            choice_selected_answer_probability: 0.8,
            scores: vec![score("a", 8, 0.9), score("b", 4, 0.5)],
            low_loser_confidence: true,
        };
        let snapshot =
            serde_json::json!({"ranking_policy":MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION});
        let ranked = GuardedMatrixAdviceOutcome::Ranked {
            ranked_choice_ids: vec!["a".into(), "b".into()],
        };
        let encoded = serde_json::to_value(&evidence).unwrap();
        assert_eq!(
            decode_trial_uncertainty(
                Some(MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION),
                Some(encoded.clone()),
                &snapshot,
                &ranked
            ),
            Ok(Some(evidence)),
        );
        assert!(decode_trial_uncertainty(None, Some(encoded.clone()), &snapshot, &ranked).is_err());
        assert!(
            decode_trial_uncertainty(
                Some(MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION),
                None,
                &snapshot,
                &ranked
            )
            .is_err()
        );
        assert!(
            decode_trial_uncertainty(
                Some(MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION),
                Some(encoded.clone()),
                &serde_json::json!({}),
                &ranked
            )
            .is_err()
        );
        let mut tampered = encoded;
        tampered["scores"][0]["feasible_minimum"] = serde_json::json!(0.0);
        assert!(
            decode_trial_uncertainty(
                Some(MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION),
                Some(tampered),
                &snapshot,
                &ranked
            )
            .is_err()
        );
        assert_eq!(
            decode_trial_uncertainty(None, None, &serde_json::json!({}), &ranked),
            Ok(None)
        );
        assert!(
            decode_trial_uncertainty(
                None,
                None,
                &serde_json::json!({"ranking_policy":"unknown"}),
                &ranked
            )
            .is_err()
        );
        assert_eq!(
            decode_trial_uncertainty(
                None,
                None,
                &snapshot,
                &GuardedMatrixAdviceOutcome::Abstained { reason: None }
            ),
            Ok(None)
        );
    }

    #[test]
    fn trial_uncertainty_migration_is_additive_and_paired() {
        let sql = include_str!("../migrations/0123_matrix_trial_uncertainty.sql");
        for required in [
            "ADD COLUMN ranking_policy_version text",
            "ADD COLUMN trial_uncertainty jsonb",
            "ranking_policy_version IS NULL AND trial_uncertainty IS NULL",
            "kind = 'ranked'",
            "jsonb_array_length(trial_uncertainty->'scores') = 2",
        ] {
            assert!(sql.contains(required), "missing {required}");
        }
        assert!(!sql.contains("UPDATE advisory_matrix_advice"));
        assert!(!sql.contains("DROP COLUMN"));
    }

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
