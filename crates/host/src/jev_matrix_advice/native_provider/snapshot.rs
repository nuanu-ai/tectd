use super::*;

pub(super) fn native_snapshot_matches(
    identity: &MatrixProviderIdentity,
    saved: &StoredMatrixDispatch,
    request_hash: &str,
) -> Result<bool> {
    let snapshot = &saved.configuration_snapshot;
    let policy_id = snapshot
        .get("budget_policy_id")
        .and_then(Value::as_str)
        .ok_or(Error::InvalidArguments)?;
    if policy_id.is_empty() || policy_id.len() > 256 || policy_id.trim() != policy_id {
        return Err(Error::InvalidArguments);
    }
    let legacy_strict = snapshot.get("ranking_policy").is_none()
        && identity.ranking_policy == MatrixRankingPolicy::StrictV1;
    Ok(snapshot
        == &expected_native_snapshot(identity, saved, policy_id, request_hash, legacy_strict)?)
}

fn expected_native_snapshot(
    identity: &MatrixProviderIdentity,
    saved: &StoredMatrixDispatch,
    policy_id: &str,
    request_hash: &str,
    legacy_strict: bool,
) -> Result<Value> {
    let mut snapshot = json!({
        "provider_profile_ref": identity.provider_profile_ref,
        "model_configuration": identity.model_configuration,
        "destination": identity.destination,
        "wire_version": native_wire::NATIVE_MATRIX_WIRE_VERSION,
        "budget_policy_id": policy_id,
        "request_body_length": saved.request_payload.len(),
        "request_body_sha256": request_hash,
        "budget_policy": validated_budget_header(&saved.configuration_snapshot, policy_id)?,
    });
    if !legacy_strict {
        snapshot["ranking_policy"] = json!(identity.ranking_policy.as_str());
    }
    match &saved.binding.verification {
        MatrixVerificationAuthority::ContextV2 {
            digest,
            snapshot_id,
            authority_schema,
            semantic_digest,
        } => {
            snapshot["matrix_authority"] = json!({
                "schema": "tect.context-matrix-verification/1",
                "verification_digest": digest,
                "frozen_snapshot_id": snapshot_id,
                "authority_schema": authority_schema,
                "requirements_semantic_digest": semantic_digest,
            });
            snapshot["advisory_correlation"] = json!({
                "opportunity_id": saved.dispatch.opportunity_id,
                "dispatch_id": saved.dispatch.id,
            });
        }
        MatrixVerificationAuthority::LegacyV1 { .. } => {}
        MatrixVerificationAuthority::Unverified => return Err(Error::InvalidArguments),
    }
    Ok(snapshot)
}

// Application owns signature verification and exact committed policy binding.
// The codec accepts only this known header; no arbitrary frozen fields are stripped.
pub(super) fn validated_budget_header(snapshot: &Value, policy_id: &str) -> Result<Value> {
    let header = snapshot
        .get("budget_policy")
        .and_then(Value::as_object)
        .ok_or(Error::InvalidArguments)?;
    if header.len() != 3
        || header.get("policy_id").and_then(Value::as_str) != Some(policy_id)
        || header
            .get("policy_version")
            .and_then(Value::as_i64)
            .is_none_or(|v| v <= 0)
        || !header
            .get("policy_digest")
            .and_then(Value::as_str)
            .is_some_and(|v| {
                v.len() == 64
                    && v.bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            })
    {
        return Err(Error::InvalidArguments);
    }
    Ok(Value::Object(header.clone()))
}
