use super::*;

struct SuppressionState {
    sequence: i64,
    lifecycle: String,
    owned_status: String,
    restore_status: String,
    original_change: Uuid,
    head_revision: Option<i64>,
    head_lifecycle: Option<String>,
    head_closed: Option<bool>,
}
type SuppressionStateRow = (
    i64,
    String,
    String,
    String,
    Uuid,
    Option<i64>,
    Option<String>,
    Option<bool>,
);
type ErasedOperationRow = (Uuid, Uuid, Option<i64>, Option<String>, bool);

async fn state(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
) -> Result<Option<SuppressionState>> {
    let row: Option<SuppressionStateRow> = sqlx::query_as(
        "SELECT l.erasure_sequence,l.lifecycle,l.owned_live_copies_status, \
             l.restore_safe_status,l.change_id,h.accepted_revision,h.lifecycle, \
             h.payload_erased AND NOT h.active AND NOT EXISTS(SELECT 1 FROM knowledge_bindings b \
             WHERE b.tenant_id=h.tenant_id AND b.workspace_id=h.workspace_id \
             AND b.unit_id=h.unit_id AND b.active) \
             FROM knowledge_suppression_ledger l LEFT JOIN knowledge_unit_heads h \
             ON h.tenant_id=l.tenant_id AND h.workspace_id=l.workspace_id AND h.unit_id=l.unit_id \
             WHERE l.tenant_id=$1 AND l.workspace_id=$2 AND l.unit_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(unit)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(row.map(|value| SuppressionState {
        sequence: value.0,
        lifecycle: value.1,
        owned_status: value.2,
        restore_status: value.3,
        original_change: value.4,
        head_revision: value.5,
        head_lifecycle: value.6,
        head_closed: value.7,
    }))
}

async fn export_covers(tx: &mut Transaction<'_, Postgres>, sequence: i64) -> Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM knowledge_suppression_exports e \
         JOIN durable_knowledge_capability c ON c.singleton \
         AND c.database_lineage_id=e.database_lineage_id \
         WHERE e.erasure_sequence >= $1 AND e.manifest_digest<>'')",
    )
    .bind(sequence)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)
}

async fn impact_ready(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
) -> Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM knowledge_lifecycle_effects \
         WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3 \
         AND kind='impact' AND status='ready')",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(change)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)
}

#[allow(clippy::too_many_arguments)]
async fn state_is_complete(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    expected_revision: i64,
    expected_lifecycle: KnowledgeLifecycleState,
    completion: &KnowledgeCompletionRequirement,
    value: &SuppressionState,
) -> Result<bool> {
    if value.sequence < 1
        || value.lifecycle != "erased"
        || value.owned_status != "ready"
        || value.head_revision != Some(expected_revision)
        || value.head_lifecycle.as_deref() != Some("erased")
        || value.head_closed != Some(true)
        || expected_lifecycle != KnowledgeLifecycleState::Erased
        || completion.search != KnowledgeSearchRequirement::NotRequired
        || matches!(
            completion.erasure,
            KnowledgeErasureRequirement::NotRequired
                | KnowledgeErasureRequirement::AllRetainedCopies
        )
        || completion.erasure == KnowledgeErasureRequirement::RestoreSafe
            && (value.restore_status != "ready" || !export_covers(tx, value.sequence).await?)
        || completion.impact_recorded
            && !impact_ready(tx, tenant, workspace, value.original_change).await?
    {
        return Ok(false);
    }
    Ok(
        super::erase::residual_owned_unit(tx, tenant, workspace, unit)
            .await?
            .complete,
    )
}

pub(crate) async fn qualify_begin(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request: &BeginKnowledgeChange,
) -> Result<Option<KnowledgeErasedNoChangeProof>> {
    let mut states = Vec::with_capacity(request.operation_hints.len());
    for hint in &request.operation_hints {
        states.push(match hint.unit_id {
            Some(unit) => state(tx, tenant, workspace, unit).await?,
            None => None,
        });
    }
    let suppressed = states.iter().filter(|value| value.is_some()).count();
    if suppressed == 0 {
        return Ok(None);
    }
    if suppressed != request.operation_hints.len() || !request.sources.is_empty() {
        return Err(Error::KnowledgePayloadErased);
    }
    let mut operations = Vec::with_capacity(states.len());
    for (hint, value) in request.operation_hints.iter().zip(states.iter()) {
        let unit = hint.unit_id.ok_or(Error::KnowledgePayloadErased)?;
        let expected_revision = hint
            .expected_revision
            .ok_or(Error::KnowledgePayloadErased)?;
        let expected_lifecycle = hint
            .expected_lifecycle
            .ok_or(Error::KnowledgePayloadErased)?;
        let value = value.as_ref().ok_or(Error::KnowledgePayloadErased)?;
        if hint.operation != KnowledgeLifecycleOperation::Erase
            || !state_is_complete(
                tx,
                tenant,
                workspace,
                unit,
                expected_revision,
                expected_lifecycle,
                &request.completion,
                value,
            )
            .await?
        {
            return Err(Error::KnowledgePayloadErased);
        }
        operations.push(KnowledgeErasedNoChangeOperationProof {
            operation_id: Uuid::new_v4(),
            unit_id: unit,
            expected_revision,
            expected_lifecycle,
            erasure_sequence: value.sequence,
        });
    }
    let proof = KnowledgeErasedNoChangeProof {
        completion: request.completion.clone(),
        operations,
    };
    proof.validate()?;
    Ok(Some(proof))
}

async fn promotion_owner_matches(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    run: Uuid,
    owner: &KnowledgeChangeOwner,
) -> Result<bool> {
    let KnowledgeChangeOwner::PromotionSlice {
        scope_id,
        slice_id,
        slice_revision,
    } = owner
    else {
        return Ok(true);
    };
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM native_slices s WHERE s.tenant_id=$1 \
         AND s.workspace_id=$2 AND s.scope_id=$3 AND s.id=$4 AND s.revision=$5 \
         AND s.pipeline='slice.promote-to-durable-knowledge' \
         AND s.knowledge_change_id=$6 AND s.knowledge_run_id=$7 \
         AND NOT EXISTS(SELECT 1 FROM slice_pipeline_runs r WHERE r.tenant_id=s.tenant_id \
         AND r.workspace_id=s.workspace_id AND r.slice_id=s.id))",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(scope_id)
    .bind(slice_id)
    .bind(slice_revision)
    .bind(change)
    .bind(run)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn validate_current(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    owner: &KnowledgeChangeOwner,
    change: Uuid,
    run: Uuid,
    proof: &KnowledgeErasedNoChangeProof,
) -> Result<()> {
    proof.validate()?;
    require_owner(tx, principal).await?;
    let identity_ready: bool =
        sqlx::query_scalar("SELECT public.tect_dk_database_identity_ready()")
            .fetch_one(&mut **tx)
            .await
            .map_err(storage_error)?;
    if !identity_ready
        || !promotion_owner_matches(tx, tenant, workspace, change, run, owner).await?
    {
        return Err(Error::KnowledgePayloadErased);
    }
    let row: Option<(serde_json::Value, serde_json::Value, bool)> = sqlx::query_as(
        "SELECT c.owner,r.erased_no_change_proof, \
         c.payload_erased AND r.payload_erased AND c.status='active' AND r.status='active' \
         AND r.current_phase_id='kc-result-handoff' AND r.current_phase_ordinal=12 \
         AND r.terminal_review_outcome='no_change' AND c.intent IS NULL \
         AND c.desired_outcome IS NULL AND c.sources IS NULL AND c.source_pins IS NULL \
         AND c.operation_hints IS NULL AND c.completion IS NULL AND r.baseline IS NULL \
         AND r.branch_plan IS NULL AND r.ready_to_commit IS NULL AND r.publisher_receipt IS NULL \
         AND r.erased_publisher_receipt IS NULL AND r.effects_report IS NULL \
         AND r.erased_effects_report IS NULL AND r.result IS NULL AND r.erased_result IS NULL \
         FROM knowledge_lifecycle_changes c JOIN knowledge_change_runs r \
         ON r.tenant_id=c.tenant_id AND r.workspace_id=c.workspace_id AND r.change_id=c.id \
         WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.id=$3 AND r.id=$4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(change)
    .bind(run)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some((stored_owner, stored_proof, safe_shape)) = row else {
        return Err(Error::KnowledgePayloadErased);
    };
    if !safe_shape
        || decode::<KnowledgeChangeOwner>(stored_owner)? != *owner
        || stored_proof != json(proof)?
    {
        return Err(Error::KnowledgePayloadErased);
    }
    let rows: Vec<ErasedOperationRow> = sqlx::query_as(
        "SELECT id,unit_id,expected_revision,expected_lifecycle, \
         payload_erased AND operation='erase' AND client_label IS NULL AND reason IS NULL \
         AND authority_basis IS NULL AND knowledge_kind IS NULL AND profile_ids IS NULL \
         AND qualification_basis IS NULL AND applied_event_id IS NULL AND applied_revision IS NULL \
         FROM knowledge_change_operations WHERE tenant_id=$1 AND workspace_id=$2 \
         AND change_id=$3 ORDER BY id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(change)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    if rows.len() != proof.operations.len()
        || rows.iter().any(|row| {
            !row.4
                || !proof.operations.iter().any(|operation| {
                    row.0 == operation.operation_id
                        && row.1 == operation.unit_id
                        && row.2 == Some(operation.expected_revision)
                        && row.3.as_deref() == Some("erased")
                })
        })
    {
        return Err(Error::KnowledgePayloadErased);
    }
    let attempts: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM knowledge_change_attempts WHERE tenant_id=$1 \
         AND workspace_id=$2 AND run_id=$3)+(SELECT count(*) FROM knowledge_change_outputs \
         WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if attempts != 0 {
        return Err(Error::KnowledgePayloadErased);
    }
    for operation in &proof.operations {
        let value = state(tx, tenant, workspace, operation.unit_id)
            .await?
            .ok_or(Error::KnowledgePayloadErased)?;
        if !state_is_complete(
            tx,
            tenant,
            workspace,
            operation.unit_id,
            operation.expected_revision,
            operation.expected_lifecycle,
            &proof.completion,
            &value,
        )
        .await?
            || value.sequence != operation.erasure_sequence
        {
            return Err(Error::KnowledgePayloadErased);
        }
    }
    Ok(())
}
