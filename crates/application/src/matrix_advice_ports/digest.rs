use super::*;

const ADVICE_DIGEST_DOMAIN: &[u8] = b"tect.guarded-matrix-advice/1\0";
const TRIAL_ADVICE_DIGEST_DOMAIN: &[u8] = b"tect.guarded-matrix-advice/robust-trial-v1\0";

/// Versioned trial digest additionally binds the exact occurrence, dispatch,
/// response hash, and all reviewable uncertainty. Strict digests are unchanged.
pub fn canonical_matrix_trial_advice_digest(
    binding: &MatrixProviderBinding,
    outcome: &GuardedMatrixAdviceOutcome,
    evidence: &MatrixTrialRankingEvidence,
    opportunity_id: Uuid,
    dispatch_id: Uuid,
    response_payload_sha256: &str,
) -> Result<String> {
    let GuardedMatrixAdviceOutcome::Ranked { ranked_choice_ids } = outcome else {
        return Err(Error::InvalidArguments);
    };
    if opportunity_id.is_nil() || dispatch_id.is_nil() || !is_digest(response_payload_sha256) {
        return Err(Error::InvalidArguments);
    }
    evidence.validate_ranked(ranked_choice_ids)?;
    let legacy_binding_digest = canonical_matrix_advice_digest(binding, outcome)?;
    let material = serde_json::json!({
        "schema":"tect.guarded-matrix-advice/robust-trial-v1",
        "binding_and_outcome_digest":legacy_binding_digest,
        "opportunity_id":opportunity_id,
        "dispatch_id":dispatch_id,
        "response_payload_sha256":response_payload_sha256,
        "trial_evidence":evidence,
    });
    let mut hasher = Sha256::new();
    hasher.update(TRIAL_ADVICE_DIGEST_DOMAIN);
    hasher.update(serde_json::to_vec(&material).map_err(|_| Error::InternalInvariant)?);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Stable, domain-separated SHA-256. JSON object keys are serialized in sorted
/// order by serde_json; array order (including rank order) is retained.
pub fn canonical_matrix_advice_digest(
    binding: &MatrixProviderBinding,
    outcome: &GuardedMatrixAdviceOutcome,
) -> Result<String> {
    let outcome = match outcome {
        GuardedMatrixAdviceOutcome::Ranked { ranked_choice_ids } => {
            serde_json::json!({"status": "ranked", "ranked_choice_ids": ranked_choice_ids})
        }
        GuardedMatrixAdviceOutcome::Abstained { reason } => {
            serde_json::json!({"status": "abstained", "reason": reason})
        }
        GuardedMatrixAdviceOutcome::Rejected { reason } => {
            serde_json::json!({"status": "rejected", "reason": reason})
        }
    };
    let mut material = serde_json::json!({
        "schema_version": match binding.verification {
            crate::MatrixVerificationAuthority::Unverified => 1,
            crate::MatrixVerificationAuthority::LegacyV1 { .. } => 2,
            crate::MatrixVerificationAuthority::ContextV2 { .. } => 3,
        },
        "binding": {
            "task_id": binding.task_id,
            "task_revision": binding.task_revision,
            "input_digest": binding.input_digest,
            "choice_set_id": binding.choice_set_id,
            "choice_set_version": binding.choice_set_version,
            "choice_set_digest": binding.choice_set_digest,
            "evaluation_digest": binding.evaluation_digest,
        },
        "outcome": outcome,
    });
    if let Some(digest) = binding.verification.digest() {
        if !is_digest(digest) {
            return Err(Error::InvalidArguments);
        }
        material["binding"]["verification_digest"] = serde_json::json!(digest);
    }
    if let crate::MatrixVerificationAuthority::ContextV2 {
        snapshot_id,
        authority_schema,
        semantic_digest,
        ..
    } = &binding.verification
    {
        if snapshot_id.is_nil()
            || authority_schema != tect_domain::MATRIX_REQUIREMENTS_SCHEMA
            || !is_digest(semantic_digest)
        {
            return Err(Error::InvalidArguments);
        }
        material["binding"]["frozen_snapshot_id"] = serde_json::json!(snapshot_id);
        material["binding"]["authority_schema"] = serde_json::json!(authority_schema);
        material["binding"]["requirements_semantic_digest"] = serde_json::json!(semantic_digest);
    }
    let encoded = serde_json::to_vec(&material).map_err(|_| Error::InternalInvariant)?;
    let mut hasher = Sha256::new();
    hasher.update(ADVICE_DIGEST_DOMAIN);
    hasher.update(encoded);
    Ok(format!("{:x}", hasher.finalize()))
}

pub(super) fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
