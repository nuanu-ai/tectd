use super::*;

#[derive(PartialEq, Eq)]
pub(super) enum MatrixChoiceStatus {
    Current,
    VerificationStale,
}

pub(super) fn supported_dispatch_opportunity(opportunity: &AdvisoryOpportunity) -> bool {
    matches!(
        (opportunity.capability, opportunity.decision_point),
        (
            AdvisoryCapability::ScopeDecomposition,
            AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection
        ) | (
            AdvisoryCapability::EngineeringProfile,
            AdvisoryDecisionPoint::EngineeringProfileBeforeSelection
        )
    )
}

pub(super) async fn require_current_matrix_choice(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity: &AdvisoryOpportunity,
) -> Result<MatrixChoiceStatus> {
    if opportunity.capability != AdvisoryCapability::EngineeringProfile
        || opportunity.decision_point != AdvisoryDecisionPoint::EngineeringProfileBeforeSelection
        || opportunity.workspace_id != workspace
        || opportunity.target_kind != "matrix_task"
        || opportunity.matrix_task_revision.is_none()
        || opportunity.matrix_task_revision != opportunity.work_revision
    {
        return Err(Error::InputConflict);
    }
    let task_id = opportunity.target_id.ok_or(Error::InputConflict)?;
    let expected_digest = opportunity
        .matrix_choice_set_digest
        .as_deref()
        .ok_or(Error::InputConflict)?;
    // The head lock serializes dispatch against an owner advancing the task.
    let current_revision: Option<i64> = sqlx::query_scalar(
        "SELECT current_revision FROM matrix_tasks \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(task_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if current_revision != opportunity.matrix_task_revision {
        return Err(Error::StaleContext);
    }
    let revision_binding: Option<(Option<String>, String)> = sqlx::query_as(
        "SELECT choice_set_digest,input_digest FROM matrix_task_revisions \
         WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3 AND revision=$4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(task_id)
    .bind(current_revision.ok_or(Error::StaleContext)?)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some((choice_digest, input_digest)) = revision_binding else {
        return Err(Error::StaleContext);
    };
    if choice_digest.as_deref() != Some(expected_digest) {
        return Err(Error::StaleContext);
    }
    let Some(expected_verification) = opportunity.matrix_verification_digest.as_deref() else {
        return Ok(MatrixChoiceStatus::VerificationStale);
    };
    // The newest exact-revision verification is authoritative. Never fall back
    // to an older header if its replacement is stale or its evidence expired.
    let latest = sqlx::query(
        "SELECT v.id,v.record_digest,v.input_digest,v.schema,v.frozen_snapshot_id, \
                v.requirements_semantic_digest,v.authority_schema, \
                b.snapshot_id AS bound_snapshot_id,b.semantic_digest AS bound_semantic_digest, \
                b.authority_schema AS bound_authority_schema \
         FROM matrix_verifications v LEFT JOIN matrix_task_requirements_bindings b \
           ON (b.tenant_id,b.workspace_id,b.task_id,b.revision)= \
              (v.tenant_id,v.workspace_id,v.task_id,v.task_revision) \
         WHERE v.tenant_id=$1 AND v.workspace_id=$2 AND v.task_id=$3 AND v.task_revision=$4 \
         ORDER BY v.verified_at DESC,v.id DESC LIMIT 1",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(task_id)
    .bind(current_revision.ok_or(Error::StaleContext)?)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some(latest) = latest else {
        return Ok(MatrixChoiceStatus::VerificationStale);
    };
    let verification_id: Uuid = latest.try_get("id").map_err(storage_error)?;
    let verification_digest: String = latest.try_get("record_digest").map_err(storage_error)?;
    let verified_input_digest: String = latest.try_get("input_digest").map_err(storage_error)?;
    let snapshot: Option<Uuid> = latest
        .try_get("frozen_snapshot_id")
        .map_err(storage_error)?;
    let semantic: Option<String> = latest
        .try_get("requirements_semantic_digest")
        .map_err(storage_error)?;
    let authority_schema: Option<String> =
        latest.try_get("authority_schema").map_err(storage_error)?;
    if verification_digest != expected_verification
        || verified_input_digest != input_digest
        || latest
            .try_get::<String, _>("schema")
            .map_err(storage_error)?
            != "tect.context-matrix-verification/1"
        || snapshot.is_none()
        || snapshot != latest.try_get("bound_snapshot_id").map_err(storage_error)?
        || semantic.is_none()
        || semantic
            != latest
                .try_get("bound_semantic_digest")
                .map_err(storage_error)?
        || authority_schema.as_deref() != Some("tect.matrix-requirements/1")
        || authority_schema
            != latest
                .try_get("bound_authority_schema")
                .map_err(storage_error)?
    {
        return Ok(MatrixChoiceStatus::VerificationStale);
    }
    let bindings_valid: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM matrix_verification_bindings \
          WHERE tenant_id=$1 AND workspace_id=$2 AND verification_id=$3) \
         AND NOT EXISTS (SELECT 1 FROM matrix_verification_bindings \
          WHERE tenant_id=$1 AND workspace_id=$2 AND verification_id=$3 \
            AND (validation_outcome <> 'accepted' OR expires_at <= \
              EXTRACT(EPOCH FROM pg_catalog.clock_timestamp())))",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(verification_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if !bindings_valid {
        return Ok(MatrixChoiceStatus::VerificationStale);
    }
    Ok(MatrixChoiceStatus::Current)
}

pub(super) async fn require_v2_matrix_dispatch_payload(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity: &AdvisoryOpportunity,
    input: &AdvisoryDispatchAuthorization,
) -> Result<()> {
    let task_id = opportunity.target_id.ok_or(Error::InputConflict)?;
    let revision = opportunity
        .matrix_task_revision
        .ok_or(Error::InputConflict)?;
    let digest = opportunity
        .matrix_verification_digest
        .as_deref()
        .ok_or(Error::StaleContext)?;
    let binding = sqlx::query(
        "SELECT b.snapshot_id,b.semantic_digest,b.authority_schema \
         FROM matrix_task_requirements_bindings b JOIN matrix_verifications v \
           ON (v.tenant_id,v.workspace_id,v.task_id,v.task_revision,v.frozen_snapshot_id, \
               v.requirements_semantic_digest,v.authority_schema)= \
              (b.tenant_id,b.workspace_id,b.task_id,b.revision,b.snapshot_id, \
               b.semantic_digest,b.authority_schema) \
         WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.task_id=$3 AND b.revision=$4 \
           AND v.record_digest=$5 AND v.schema='tect.context-matrix-verification/1'",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(task_id)
    .bind(revision)
    .bind(digest)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?
    .ok_or(Error::StaleContext)?;
    let snapshot_id: Uuid = binding.try_get("snapshot_id").map_err(storage_error)?;
    let semantic: String = binding.try_get("semantic_digest").map_err(storage_error)?;
    let authority_schema: String = binding.try_get("authority_schema").map_err(storage_error)?;
    let expected_authority = serde_json::json!({
        "schema": "tect.context-matrix-verification/1",
        "verification_digest": digest,
        "frozen_snapshot_id": snapshot_id,
        "authority_schema": authority_schema,
        "requirements_semantic_digest": semantic,
    });
    if input.configuration_snapshot.get("matrix_authority") != Some(&expected_authority)
        || input.configuration_snapshot.get("advisory_correlation")
            != Some(&serde_json::json!({
                "opportunity_id": input.opportunity_id,
                "dispatch_id": input.dispatch_id,
            }))
    {
        return Err(Error::InputConflict);
    }
    let body: serde_json::Value =
        serde_json::from_slice(&input.request_payload).map_err(|_| Error::InputConflict)?;
    let context = serde_json::json!({
        "schema": "tect.context-matrix-verification/1",
        "frozen_snapshot_id": snapshot_id,
        "authority_schema": authority_schema,
        "requirements_semantic_digest": semantic,
    });
    if !matches_verified_matrix_payload(input, &body, digest, &context) {
        return Err(Error::InputConflict);
    }
    Ok(())
}

fn matches_verified_matrix_payload(
    input: &AdvisoryDispatchAuthorization,
    body: &serde_json::Value,
    digest: &str,
    context: &serde_json::Value,
) -> bool {
    if body.pointer("/state/binding/verification_digest") != Some(&serde_json::json!(digest))
        || body.pointer("/state/binding/context") != Some(context)
        || body.pointer("/state/binding/evaluation_digest")
            != Some(&serde_json::json!(input.material_digest))
    {
        return false;
    }
    match body.pointer("/state/contract").and_then(serde_json::Value::as_str) {
        Some("tect.context-matrix-verified-evaluation/1") => true,
        Some("tect.matrix-typesafe-native/1") => {
            let snapshot = &input.configuration_snapshot;
            let policy = snapshot.get("ranking_policy").and_then(serde_json::Value::as_str);
            let body_policy = body.pointer("/state/ranking_policy").and_then(serde_json::Value::as_str);
            snapshot.get("wire_version") == Some(&serde_json::json!("tect.matrix-typesafe-native/1"))
                && matches!(policy, Some(tect_domain::MATRIX_NATIVE_RANKING_POLICY_VERSION)
                    | Some(tect_domain::MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION))
                && body_policy == if policy == Some(tect_domain::MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION) {
                    policy
                } else {
                    None
                }
                && body.pointer("/state/ranking_policy").is_none_or(|value| value.is_string())
                && snapshot.get("destination")
                    == Some(&serde_json::json!("https://api.typesafe.ai/v1/systemone"))
                && snapshot.pointer("/provider_profile_ref/id")
                    == Some(&serde_json::json!(input.provider))
                && snapshot.pointer("/model_configuration/model")
                    == Some(&serde_json::json!(input.model))
        }
        _ => false,
    }
}

#[cfg(test)]
mod payload_tests {
    use super::*;

    fn input() -> AdvisoryDispatchAuthorization {
        AdvisoryDispatchAuthorization {
            dispatch_id: Uuid::new_v4(),
            opportunity_id: Uuid::new_v4(),
            predecessor_dispatch_id: None,
            attempt_number: 1,
            retry_basis: AdvisoryRetryBasis::Initial,
            provider: "local-profile".into(),
            model: "jev-1.13.0".into(),
            configuration_snapshot: serde_json::json!({
                "wire_version": "tect.matrix-typesafe-native/1",
                "ranking_policy": tect_domain::MATRIX_NATIVE_RANKING_POLICY_VERSION,
                "destination": "https://api.typesafe.ai/v1/systemone",
                "provider_profile_ref": {"id": "local-profile"},
                "model_configuration": {"model": "jev-1.13.0"},
            }),
            configuration_digest: "a".repeat(64),
            material_digest: "b".repeat(64),
            payload_digest: "c".repeat(64),
            request_payload: vec![1],
        }
    }

    fn payload(contract: &str) -> serde_json::Value {
        serde_json::json!({"state": {
            "contract": contract,
            "binding": {
                "verification_digest": "verified",
                "context": {"schema": "tect.context-matrix-verification/1"},
                "evaluation_digest": "b".repeat(64),
            }
        }})
    }

    #[test]
    fn native_requires_exact_provider_wire_identity_and_verified_binding() {
        let context = serde_json::json!({"schema": "tect.context-matrix-verification/1"});
        let input = input();
        let body = payload("tect.matrix-typesafe-native/1");
        assert!(matches_verified_matrix_payload(&input, &body, "verified", &context));
        let mut unsigned = input.clone();
        unsigned.configuration_snapshot.as_object_mut().unwrap().remove("ranking_policy");
        assert!(!matches_verified_matrix_payload(&unsigned, &body, "verified", &context));
        let mut trial = input.clone();
        trial.configuration_snapshot["ranking_policy"] =
            serde_json::json!(tect_domain::MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION);
        assert!(!matches_verified_matrix_payload(&trial, &body, "verified", &context));
        let mut trial_body = body.clone();
        trial_body["state"]["ranking_policy"] =
            serde_json::json!(tect_domain::MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION);
        assert!(matches_verified_matrix_payload(&trial, &trial_body, "verified", &context));
        assert!(!matches_verified_matrix_payload(&input, &trial_body, "verified", &context));
        trial.configuration_snapshot["ranking_policy"] = serde_json::json!("other-policy");
        assert!(!matches_verified_matrix_payload(&trial, &trial_body, "verified", &context));

        for (field, wrong) in [
            ("wire_version", "other-wire"),
            ("destination", "https://example.invalid/other"),
        ] {
            let mut changed = input.clone();
            changed.configuration_snapshot[field] = serde_json::json!(wrong);
            assert!(!matches_verified_matrix_payload(&changed, &body, "verified", &context));
        }
        let mut changed = input.clone();
        changed.configuration_snapshot["provider_profile_ref"]["id"] = serde_json::json!("other");
        assert!(!matches_verified_matrix_payload(&changed, &body, "verified", &context));
        changed = input.clone();
        changed.configuration_snapshot["model_configuration"]["model"] = serde_json::json!("other");
        assert!(!matches_verified_matrix_payload(&changed, &body, "verified", &context));
        let mut missing = body.clone();
        missing["state"]["binding"].as_object_mut().unwrap().remove("verification_digest");
        assert!(!matches_verified_matrix_payload(&input, &missing, "verified", &context));
        missing = body.clone();
        missing["state"]["binding"].as_object_mut().unwrap().remove("context");
        assert!(!matches_verified_matrix_payload(&input, &missing, "verified", &context));
        missing = body.clone();
        missing["state"]["binding"].as_object_mut().unwrap().remove("evaluation_digest");
        assert!(!matches_verified_matrix_payload(&input, &missing, "verified", &context));
        assert!(!matches_verified_matrix_payload(&input, &payload("other-contract"), "verified", &context));
    }

    #[test]
    fn existing_verified_evaluation_contract_remains_accepted() {
        let context = serde_json::json!({"schema": "tect.context-matrix-verification/1"});
        let body = payload("tect.context-matrix-verified-evaluation/1");
        assert!(matches_verified_matrix_payload(&input(), &body, "verified", &context));
    }
}
