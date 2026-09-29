use super::*;
use tect_domain::{
    MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION, MATRIX_TRIAL_POLICY_ID,
    MatrixTrialCandidateEvidence, NativeMatrixScoreDistribution, matrix_trial_policy_digest,
};

fn fixture() -> GuardedMatrixAdviceRecord {
    let binding = MatrixProviderBinding {
        task_id: Uuid::new_v4(),
        task_revision: 1,
        input_digest: "a".repeat(64),
        choice_set_id: "set".into(),
        choice_set_version: 1,
        choice_set_digest: "b".repeat(64),
        evaluation_digest: "c".repeat(64),
        verification: crate::MatrixVerificationAuthority::ContextV2 {
            digest: "d".repeat(64),
            snapshot_id: Uuid::new_v4(),
            authority_schema: tect_domain::MATRIX_REQUIREMENTS_SCHEMA.into(),
            semantic_digest: "e".repeat(64),
        },
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
    let outcome = GuardedMatrixAdviceOutcome::Ranked {
        ranked_choice_ids: vec!["a".into(), "b".into()],
    };
    let opportunity_id = Uuid::new_v4();
    let dispatch_id = Uuid::new_v4();
    let raw = b"opaque-trial-response".to_vec();
    let response_payload_sha256 = format!("{:x}", Sha256::digest(&raw));
    let advice_digest = canonical_matrix_trial_advice_digest(
        &binding,
        &outcome,
        &evidence,
        opportunity_id,
        dispatch_id,
        &response_payload_sha256,
    )
    .unwrap();
    GuardedMatrixAdviceRecord {
        opportunity_id,
        dispatch_id,
        opportunity_material_digest: binding.evaluation_digest.clone(),
        binding,
        provider_profile_ref: AdvisoryProviderProfileRef {
            id: "profile".into(),
        },
        model_configuration: AdvisoryModelConfiguration {
            model: "model".into(),
        },
        raw_response_payload: raw,
        response_payload_sha256,
        advice_digest,
        outcome,
        trial_evidence: Some(evidence),
    }
}

fn validate(record: &GuardedMatrixAdviceRecord) -> Result<()> {
    record.validate_for(
        record.opportunity_id,
        record.dispatch_id,
        &record.binding,
        &record.provider_profile_ref,
        &record.model_configuration,
        &MatrixAdviceEligibility::EligibleForAdvice {
            candidate_ids: vec!["a".into(), "b".into()],
        },
    )
}

#[test]
fn trial_digest_binds_occurrence_dispatch_response_and_uncertainty() {
    let record = fixture();
    assert_eq!(validate(&record), Ok(()));
    let strict_digest = canonical_matrix_advice_digest(&record.binding, &record.outcome).unwrap();
    assert_ne!(record.advice_digest, strict_digest);
    let mut changed = record.clone();
    changed.dispatch_id = Uuid::new_v4();
    assert_eq!(validate(&changed), Err(Error::InvalidArguments));
    changed = record.clone();
    changed.trial_evidence.as_mut().unwrap().scores[1].score_confidence = 0.4;
    assert_eq!(validate(&changed), Err(Error::InvalidArguments));
    changed = record.clone();
    changed.trial_evidence = None;
    assert_eq!(validate(&changed), Err(Error::InvalidArguments));
    changed = record.clone();
    changed.advice_digest = strict_digest;
    assert_eq!(validate(&changed), Err(Error::InvalidArguments));
}
