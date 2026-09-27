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
    if body.pointer("/state/binding/verification_digest") != Some(&serde_json::json!(digest))
        || body.pointer("/state/binding/context") != Some(&context)
        || body.pointer("/state/binding/evaluation_digest")
            != Some(&serde_json::json!(input.material_digest))
        || body.pointer("/state/contract")
            != Some(&serde_json::json!(
                "tect.context-matrix-verified-evaluation/1"
            ))
    {
        return Err(Error::InputConflict);
    }
    Ok(())
}
