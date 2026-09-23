#[derive(sqlx::FromRow)]
struct AdviceRow {
    candidate_set_id: Uuid,
    advice_id: String,
    dispatch_id: Uuid,
    dispatch_material_digest: String,
    config_revision: i64,
    source_digest: String,
    manifest_digest: String,
    eligible_set_digest: String,
    request_digest: String,
    normalized_answers_digest: String,
    aggregate_payload: serde_json::Value,
}

#[derive(sqlx::FromRow)]
struct DispositionRow {
    opportunity_id: Uuid,
    candidate_set_id: Uuid,
    actor_id: Uuid,
    session_id: Uuid,
    disposition_id: Uuid,
    request_id: Uuid,
    advice_id: String,
    revision: i64,
    predecessor_id: Option<Uuid>,
    action: String,
    selected_alternative_id: Option<String>,
    aggregate_payload: serde_json::Value,
}

fn disposition_action(value: ScopeDispositionAction) -> &'static str {
    match value {
        ScopeDispositionAction::Accept => "accept",
        ScopeDispositionAction::RejectAll => "reject_all",
        ScopeDispositionAction::SupersedeWithDeterministicChoice => {
            "supersede_with_deterministic_choice"
        }
    }
}

fn preservation_status(value: &ScopePreservationStatus) -> &'static str {
    match value {
        ScopePreservationStatus::Passed => "passed",
        ScopePreservationStatus::Failed => "failed",
    }
}

async fn lock_scope_key(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    kind: &str,
    id: impl std::fmt::Display,
) -> Result<()> {
    let key = format!("tect.scope-advisory:{tenant}:{workspace}:{kind}:{id}");
    sqlx::query("SELECT pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended($1,0))")
        .bind(key)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    Ok(())
}

async fn actor_session_exists(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    actor: Uuid,
    session: Uuid,
) -> Result<bool> {
    let _ = (tenant, workspace);
    sqlx::query_scalar("SELECT public.tect_dk_session_principal($1)=$2")
        .bind(session)
        .bind(actor)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)
}

async fn require_current_opportunity_config(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity: Uuid,
    candidate_set_id: Uuid,
    config_revision: i64,
) -> Result<()> {
    let current: Option<i64> = sqlx::query_scalar(
        "SELECT o.config_revision FROM advisory_opportunity o \
         JOIN advisory_workspace_config c \
           ON (c.tenant_id,c.workspace_id)=(o.tenant_id,o.workspace_id) \
         JOIN advisory_workspace_config_history h \
           ON (h.tenant_id,h.workspace_id,h.revision)=(c.tenant_id,c.workspace_id,c.revision) \
         WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.id=$3 \
           AND o.scope_id IS NULL AND o.work_item_kind='scope_candidate_set' AND o.work_item_id=$4 \
           AND o.config_revision=$5 AND c.revision=$5 \
           AND o.capability='scope_decomposition' \
           AND o.decision_point='scope.decomposition.before_selection' \
           AND o.state='advised' AND o.primary_reason='provider_response' \
           AND c.mode='optional' AND h.mode='optional' \
           AND c.provider_profile_ref IS NOT NULL AND c.model_configuration IS NOT NULL \
           AND c.provider_profile_ref=h.provider_profile_ref \
           AND c.model_configuration=h.model_configuration FOR UPDATE OF o,c",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(opportunity)
    .bind(candidate_set_id)
    .bind(config_revision)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if current != Some(config_revision) {
        return Err(Error::StaleContext);
    }
    Ok(())
}

async fn load_advice(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity: Uuid,
    expected_candidate: Option<Uuid>,
) -> Result<Option<(AdviceRow, GuardedScopeAdvice)>> {
    let row: Option<AdviceRow> = sqlx::query_as(
        "SELECT candidate_set_id,advice_id,dispatch_id,dispatch_material_digest,config_revision,source_digest,\
                manifest_digest,eligible_set_digest,request_digest,normalized_answers_digest,aggregate_payload \
         FROM advisory_scope_advice WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(opportunity)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some(row) = row else { return Ok(None) };
    if expected_candidate.is_some_and(|value| value != row.candidate_set_id) {
        return Err(Error::InputConflict);
    }
    let advice: GuardedScopeAdvice =
        serde_json::from_value(row.aggregate_payload.clone()).map_err(storage_error)?;
    let manifest = load_manifest(tx, tenant, workspace, opportunity, Some(row.candidate_set_id))
        .await?
        .ok_or(Error::StorageUnavailable)?;
    validate_guarded_advice_binding(&Sha256ScopeDigest, &manifest, &advice)?;
    if advice.id.0 != row.advice_id
        || advice.source_digest != row.source_digest
        || advice.manifest_digest != row.manifest_digest
        || advice.eligible_set_digest != row.eligible_set_digest
        || advice.request_digest != row.request_digest
        || advice.normalized_answers_digest != row.normalized_answers_digest
    {
        return Err(Error::StorageUnavailable);
    }
    Ok(Some((row, advice)))
}

async fn load_disposition(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    row: DispositionRow,
) -> Result<ScopeDispositionRevision> {
    let revision: ScopeDispositionRevision =
        serde_json::from_value(row.aggregate_payload).map_err(storage_error)?;
    let manifest = load_manifest(tx, tenant, workspace, row.opportunity_id, Some(row.candidate_set_id))
        .await?
        .ok_or(Error::StorageUnavailable)?;
    let (_, advice) = load_advice(tx, tenant, workspace, row.opportunity_id, Some(row.candidate_set_id))
        .await?
        .ok_or(Error::StorageUnavailable)?;
    revision.validate(&Sha256ScopeDigest, &manifest, &advice)?;
    if revision.id != row.disposition_id
        || revision.request_id != row.request_id
        || revision.advice_id.0 != row.advice_id
        || revision.revision != row.revision
        || revision.supersedes_id != row.predecessor_id
        || disposition_action(revision.action) != row.action
        || revision.selected_id.as_ref().map(|id| &id.0) != row.selected_alternative_id.as_ref()
    {
        return Err(Error::StorageUnavailable);
    }
    Ok(revision)
}
