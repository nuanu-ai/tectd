use super::*;

pub(crate) async fn observe(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Uuid,
    request: &ObserveKnowledgeMaintenanceSignal,
) -> Result<ObserveKnowledgeMaintenanceOutcome> {
    require_identity(tx).await?;
    require_owner(tx, principal).await?;
    let payload = json(request)?;
    if let Some(task) = replay(
        tx,
        tenant,
        workspace,
        principal,
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(ObserveKnowledgeMaintenanceOutcome::Replay(task));
    }
    publisher_lock(tx).await?;
    crate::durable_knowledge::lock_state(tx, tenant, workspace).await?;
    if let Some(task) = replay(
        tx,
        tenant,
        workspace,
        principal,
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(ObserveKnowledgeMaintenanceOutcome::Replay(task));
    }
    if matches!(
        request.basis,
        KnowledgeMaintenanceBasis::ApplicationFailed { .. }
    ) {
        super::reconcile_unit_consumers(tx, tenant, workspace, request.unit_id).await?;
    }
    validate_observation(tx, tenant, workspace, principal, request).await?;
    let (task, created) = ensure_signal_task(
        tx,
        tenant,
        workspace,
        principal,
        Some(session),
        request.unit_id,
        request.unit_revision,
        &request.basis,
        Some(request.request_id),
        true,
    )
    .await?;
    let result = if created {
        ObserveKnowledgeMaintenanceOutcome::Created(task.clone())
    } else {
        ObserveKnowledgeMaintenanceOutcome::Existing(task.clone())
    };
    let receipt = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO knowledge_maintenance_command_receipts \
         (id,tenant_id,workspace_id,operation,request_id,actor_principal_id,actor_session_id, \
          unit_id,request_payload,result_payload) VALUES($1,$2,$3,'observe',$4,$5,$6,$7,$8,$9)",
    )
    .bind(receipt)
    .bind(tenant)
    .bind(workspace)
    .bind(request.request_id)
    .bind(principal)
    .bind(session)
    .bind(request.unit_id)
    .bind(payload)
    .bind(json(&result)?)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    register_copy(
        tx,
        tenant,
        workspace,
        request.unit_id,
        "maintenance_receipt",
        "knowledge_maintenance_command_receipts",
        receipt,
        request.unit_revision,
    )
    .await?;
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn ensure_signal_task(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Option<Uuid>,
    unit: Uuid,
    revision: i64,
    basis: &KnowledgeMaintenanceBasis,
    request: Option<Uuid>,
    invalidate_generation: bool,
) -> Result<(KnowledgeMaintenanceTask, bool)> {
    basis.validate()?;
    let reason = enum_text(&basis.reason())?;
    let basis_digest = digest(basis)?;
    let signal = Uuid::new_v4();
    let stored_signal: Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_maintenance_signals \
         (id,tenant_id,workspace_id,request_id,unit_id,unit_revision,reason,basis,basis_digest, \
          actor_principal_id,actor_session_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) \
         ON CONFLICT(tenant_id,workspace_id,unit_id,unit_revision,reason,basis_digest) \
         WHERE NOT payload_erased DO UPDATE SET id=knowledge_maintenance_signals.id RETURNING id",
    )
    .bind(signal)
    .bind(tenant)
    .bind(workspace)
    .bind(request)
    .bind(unit)
    .bind(revision)
    .bind(reason)
    .bind(json(basis)?)
    .bind(&basis_digest)
    .bind(principal)
    .bind(session)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let task: Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_maintenance_tasks(id,tenant_id,workspace_id,signal_id) \
         VALUES($1,$2,$3,$4) ON CONFLICT(tenant_id,workspace_id,signal_id) \
         DO UPDATE SET signal_id=EXCLUDED.signal_id RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(tenant)
    .bind(workspace)
    .bind(stored_signal)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if stored_signal == signal {
        register_copy(
            tx,
            tenant,
            workspace,
            unit,
            "maintenance_signal",
            "knowledge_maintenance_signals",
            signal,
            revision,
        )
        .await?;
        register_copy(
            tx,
            tenant,
            workspace,
            unit,
            "maintenance_task",
            "knowledge_maintenance_tasks",
            task,
            revision,
        )
        .await?;
        if invalidate_generation {
            invalidate_generation_locked(tx, tenant, workspace).await?;
        }
    }
    Ok((
        super::query::load_task(tx, tenant, workspace, task).await?,
        stored_signal == signal,
    ))
}

async fn validate_observation(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    request: &ObserveKnowledgeMaintenanceSignal,
) -> Result<()> {
    let unit = crate::knowledge_lifecycle::unit(
        tx,
        tenant,
        workspace,
        principal,
        &KnowledgeUnitQuery {
            unit_id: request.unit_id,
            revision: Some(request.unit_revision),
            fragment: None,
        },
    )
    .await?
    .ok_or(Error::NotFound)?;
    match (&request.basis, unit) {
        (
            KnowledgeMaintenanceBasis::SourceChanged {
                source_iri,
                accepted_digest,
                ..
            },
            KnowledgeUnitResponse::Document(_) | KnowledgeUnitResponse::LegacyConstraint(_),
        ) => {
            if super::source::verified_logical_uri(
                tx,
                tenant,
                workspace,
                request.unit_id,
                request.unit_revision,
                source_iri,
                accepted_digest,
            )
            .await?
            .is_none()
            {
                return Err(Error::ContextChanged);
            }
        }
        (
            KnowledgeMaintenanceBasis::ApplicationFailed { consumer_ref, .. },
            KnowledgeUnitResponse::Document(_) | KnowledgeUnitResponse::LegacyConstraint(_),
        ) => {
            if !registered_consumers(
                tx,
                tenant,
                workspace,
                request.unit_id,
                request.unit_revision,
            )
            .await?
            .iter()
            .any(|consumer| consumer.consumer_ref == *consumer_ref)
            {
                return Err(Error::ContextChanged);
            }
        }
        (
            KnowledgeMaintenanceBasis::OperatorRequested { subject_ref, .. },
            KnowledgeUnitResponse::Document(value),
        ) if subject_ref == &value.unit_iri || subject_ref == &value.revision_iri => {}
        (
            KnowledgeMaintenanceBasis::OperatorRequested { subject_ref, .. },
            KnowledgeUnitResponse::LegacyConstraint(value),
        ) if subject_ref == &value.unit_iri || subject_ref == &value.revision_iri => {}
        (.., KnowledgeUnitResponse::PayloadErased(_)) => return Err(Error::KnowledgePayloadErased),
        _ => return Err(Error::ContextChanged),
    }
    Ok(())
}

async fn replay(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    request: Uuid,
    payload: &serde_json::Value,
) -> Result<Option<KnowledgeMaintenanceTask>> {
    let row: Option<(Uuid, Option<serde_json::Value>, bool, Uuid)> = sqlx::query_as(
        "SELECT actor_principal_id,request_payload,payload_erased,id \
         FROM knowledge_maintenance_command_receipts WHERE tenant_id=$1 AND workspace_id=$2 \
         AND operation='observe' AND request_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    match row {
        Some((actor, _, _, _)) if actor != principal => Err(Error::Forbidden),
        Some((_, _, true, _)) => Err(Error::KnowledgePayloadErased),
        Some((_, Some(stored), false, receipt)) if stored == *payload => {
            let task: Uuid = sqlx::query_scalar(
                "SELECT c.row_id FROM knowledge_owned_copies c WHERE c.tenant_id=$1 \
                 AND c.workspace_id=$2 AND c.relation_name='knowledge_maintenance_command_receipts' \
                 AND c.row_id=$3 AND NOT c.redacted LIMIT 1",
            )
            .bind(tenant)
            .bind(workspace)
            .bind(receipt)
            .fetch_optional(&mut **tx)
            .await
            .map_err(storage_error)?
            .ok_or(Error::InternalInvariant)?;
            let _ = task;
            let result: Option<serde_json::Value> = sqlx::query_scalar(
                "SELECT result_payload FROM knowledge_maintenance_command_receipts \
                 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
            )
            .bind(tenant)
            .bind(workspace)
            .bind(receipt)
            .fetch_one(&mut **tx)
            .await
            .map_err(storage_error)?;
            let stored: ObserveKnowledgeMaintenanceOutcome =
                decode(result.ok_or(Error::InternalInvariant)?)?;
            Ok(Some(match stored {
                ObserveKnowledgeMaintenanceOutcome::Created(task)
                | ObserveKnowledgeMaintenanceOutcome::Existing(task)
                | ObserveKnowledgeMaintenanceOutcome::Replay(task) => task,
            }))
        }
        Some((_, Some(_), false, _)) => Err(Error::InputConflict),
        Some(_) => Err(Error::InternalInvariant),
        None => Ok(None),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn register_copy(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    kind: &str,
    relation: &str,
    row: Uuid,
    revision: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO knowledge_owned_copies \
         (id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,source_revision) \
         VALUES(pg_catalog.gen_random_uuid(),$1,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(unit)
    .bind(kind)
    .bind(relation)
    .bind(row)
    .bind(revision)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(super) async fn invalidate_generation_locked(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
) -> Result<i64> {
    let current: i64 = sqlx::query_scalar(
        "SELECT generation FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2 FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let next = current.checked_add(1).ok_or(Error::StorageUnavailable)?;
    sqlx::query(
        "UPDATE workspace_knowledge_state SET generation=$3 WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(next)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(next)
}

fn enum_text<T: Serialize>(value: &T) -> Result<String> {
    match json(value)? {
        serde_json::Value::String(value) => Ok(value),
        _ => Err(Error::InternalInvariant),
    }
}
