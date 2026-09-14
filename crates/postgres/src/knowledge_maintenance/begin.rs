use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn begin_change(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Uuid,
    request: &BeginKnowledgeMaintenanceChange,
    definition: &KnowledgeChangeDefinition,
    registry: &KnowledgeProfileRegistry,
) -> Result<BeginKnowledgeMaintenanceChangeOutcome> {
    require_identity(tx).await?;
    require_owner(tx, principal).await?;
    let payload = json(request)?;
    if let Some(value) = replay(
        tx,
        tenant,
        workspace,
        principal,
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(replay_outcome(value));
    }
    publisher_lock(tx).await?;
    crate::durable_knowledge::lock_state(tx, tenant, workspace).await?;
    if let Some(value) = replay(
        tx,
        tenant,
        workspace,
        principal,
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(replay_outcome(value));
    }
    let row: Option<(i64, String, Uuid, i64, serde_json::Value, bool)> = sqlx::query_as(
        "SELECT t.revision,t.state,s.unit_id,s.unit_revision,s.basis,t.payload_erased \
         FROM knowledge_maintenance_tasks t JOIN knowledge_maintenance_signals s \
          ON s.tenant_id=t.tenant_id AND s.workspace_id=t.workspace_id AND s.id=t.signal_id \
         WHERE t.tenant_id=$1 AND t.workspace_id=$2 AND t.id=$3 FOR UPDATE OF t",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.task_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some((task_revision, state, unit, unit_revision, basis, erased)) = row else {
        return Err(Error::NotFound);
    };
    if erased {
        return Err(Error::KnowledgePayloadErased);
    }
    if task_revision != request.task_revision
        || !matches!(state.as_str(), "needs_review" | "exhausted")
    {
        return Err(Error::StaleRevision);
    }
    let hint = &request.change.operation_hints[0];
    if hint.unit_id != Some(unit)
        || hint.expected_revision != Some(unit_revision)
        || hint.expected_lifecycle != Some(KnowledgeLifecycleState::Active)
    {
        return Err(Error::ContextChanged);
    }
    let head: Option<i64> = sqlx::query_scalar(
        "SELECT accepted_revision FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 \
         AND unit_id=$3 AND lifecycle='active' AND active AND NOT payload_erased",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(unit)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if head != Some(unit_revision) {
        return Err(Error::ContextChanged);
    }
    let review =
        current_unit_review_status(tx, tenant, workspace, principal, unit, unit_revision).await?;
    let consumers = registered_consumers(tx, tenant, workspace, unit, unit_revision).await?;
    if !super::jobs::basis_applies(
        tx,
        tenant,
        workspace,
        unit,
        unit_revision,
        &decode(basis)?,
        &review,
        &consumers,
    )
    .await?
    {
        return Err(Error::ContextChanged);
    }
    let change = crate::knowledge_lifecycle::begin(
        tx,
        tenant,
        workspace,
        principal,
        session,
        &request.change,
        definition,
        registry,
    )
    .await?;
    let context = match &change {
        BeginKnowledgeChangeOutcome::Created(value)
        | BeginKnowledgeChangeOutcome::Replay(value) => value,
    };
    sqlx::query(
        "UPDATE knowledge_maintenance_tasks SET state='linked',revision=revision+1,failure_code=NULL,change_id=$4, \
         run_id=$5,current_review=$6,affected_consumers=$7,updated_at=pg_catalog.clock_timestamp() \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.task_id)
    .bind(context.change_id)
    .bind(context.run.id)
    .bind(json(&review)?)
    .bind(json(&consumers)?)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    let task = super::query::load_task(tx, tenant, workspace, request.task_id).await?;
    let linked_context =
        crate::knowledge_lifecycle::load_context(tx, tenant, workspace, context.change_id)
            .await?
            .ok_or(Error::InternalInvariant)?;
    let linked_change = match change {
        BeginKnowledgeChangeOutcome::Created(_) => {
            BeginKnowledgeChangeOutcome::Created(Box::new(linked_context))
        }
        BeginKnowledgeChangeOutcome::Replay(_) => {
            BeginKnowledgeChangeOutcome::Replay(Box::new(linked_context))
        }
    };
    let result = BeginKnowledgeMaintenanceChangeOutcome::Created {
        task,
        change: linked_change,
    };
    let receipt = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO knowledge_maintenance_command_receipts \
         (id,tenant_id,workspace_id,operation,request_id,actor_principal_id,actor_session_id, \
          unit_id,request_payload,result_payload) VALUES($1,$2,$3,'begin_change',$4,$5,$6,$7,$8,$9)",
    )
    .bind(receipt)
    .bind(tenant)
    .bind(workspace)
    .bind(request.request_id)
    .bind(principal)
    .bind(session)
    .bind(unit)
    .bind(payload)
    .bind(json(&result)?)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    super::signal::register_copy(
        tx,
        tenant,
        workspace,
        unit,
        "maintenance_receipt",
        "knowledge_maintenance_command_receipts",
        receipt,
        unit_revision,
    )
    .await?;
    Ok(result)
}

async fn replay(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    request: Uuid,
    payload: &serde_json::Value,
) -> Result<Option<BeginKnowledgeMaintenanceChangeOutcome>> {
    let row: Option<(
        Uuid,
        Option<serde_json::Value>,
        Option<serde_json::Value>,
        bool,
    )> = sqlx::query_as(
        "SELECT actor_principal_id,request_payload,result_payload,payload_erased \
             FROM knowledge_maintenance_command_receipts WHERE tenant_id=$1 AND workspace_id=$2 \
             AND operation='begin_change' AND request_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    match row {
        Some((actor, _, _, _)) if actor != principal => Err(Error::Forbidden),
        Some((_, _, _, true)) => Err(Error::KnowledgePayloadErased),
        Some((_, Some(stored), Some(result), false)) if stored == *payload => {
            Ok(Some(decode(result)?))
        }
        Some((_, Some(_), Some(_), false)) => Err(Error::InputConflict),
        Some(_) => Err(Error::InternalInvariant),
        None => Ok(None),
    }
}

fn replay_outcome(
    value: BeginKnowledgeMaintenanceChangeOutcome,
) -> BeginKnowledgeMaintenanceChangeOutcome {
    match value {
        BeginKnowledgeMaintenanceChangeOutcome::Created { task, change }
        | BeginKnowledgeMaintenanceChangeOutcome::Replay { task, change } => {
            BeginKnowledgeMaintenanceChangeOutcome::Replay { task, change }
        }
    }
}
