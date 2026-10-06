use super::*;

pub(super) fn trial_ranked_snapshot_matches(
    snapshot: &serde_json::Value,
    request_payload: &[u8],
    record: &GuardedMatrixAdviceRecord,
) -> bool {
    let GuardedMatrixAdviceOutcome::Ranked { ranked_choice_ids } = &record.outcome else {
        return false;
    };
    let Some(evidence) = &record.trial_evidence else {
        return false;
    };
    let MatrixVerificationAuthority::ContextV2 {
        digest,
        snapshot_id,
        authority_schema,
        semantic_digest,
    } = &record.binding.verification
    else {
        return false;
    };
    let Ok(body) = serde_json::from_slice::<serde_json::Value>(request_payload) else {
        return false;
    };
    let context = serde_json::json!({
        "schema":"tect.context-matrix-verification/1",
        "frozen_snapshot_id":snapshot_id,
        "authority_schema":authority_schema,
        "requirements_semantic_digest":semantic_digest,
    });
    evidence.validate_ranked(ranked_choice_ids).is_ok()
        && snapshot.get("ranking_policy") == Some(&serde_json::json!(evidence.policy_version))
        && snapshot.get("wire_version") == Some(&serde_json::json!("tect.matrix-typesafe-native/1"))
        && snapshot.get("destination")
            == Some(&serde_json::json!("https://api.typesafe.ai/v1/systemone"))
        && snapshot.get("matrix_authority")
            == Some(&serde_json::json!({
                "schema":"tect.context-matrix-verification/1",
                "verification_digest":digest,
                "frozen_snapshot_id":snapshot_id,
                "authority_schema":authority_schema,
                "requirements_semantic_digest":semantic_digest,
            }))
        && snapshot.get("advisory_correlation")
            == Some(&serde_json::json!({
                "opportunity_id":record.opportunity_id,
                "dispatch_id":record.dispatch_id,
            }))
        && body.pointer("/state/contract")
            == Some(&serde_json::json!("tect.matrix-typesafe-native/1"))
        && body.pointer("/state/ranking_policy")
            == Some(&serde_json::json!(evidence.policy_version))
        && body.pointer("/state/binding/verification_digest") == Some(&serde_json::json!(digest))
        && body.pointer("/state/binding/evaluation_digest")
            == Some(&serde_json::json!(record.binding.evaluation_digest))
        && body.pointer("/state/binding/context") == Some(&context)
}
