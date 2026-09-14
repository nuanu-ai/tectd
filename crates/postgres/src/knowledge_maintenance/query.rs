use super::*;
use sqlx::{PgConnection, Row, postgres::PgRow};

pub(crate) async fn context(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    query: &KnowledgeMaintenanceQuery,
    method: &PipelineInstructionSnapshot,
) -> Result<KnowledgeMaintenanceContext> {
    require_identity(tx).await?;
    require_owner(tx, principal).await?;
    let generation: i64 = sqlx::query_scalar(
        "SELECT generation FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let states = query
        .states
        .iter()
        .map(enum_text)
        .collect::<Result<Vec<_>>>()?;
    let rows = sqlx::query(
        "SELECT t.id,t.revision,t.state,t.attempts,t.failure_code, \
         to_char(t.next_retry_at,'YYYY-MM-DD\"T\"HH24:MI:SS.USOF'),t.change_id,t.run_id, \
         t.current_review,t.affected_consumers,t.terminal_evidence,t.payload_erased, \
         s.id,s.unit_id,s.unit_revision,s.reason,s.basis,s.basis_digest, \
         to_char(s.observed_at,'YYYY-MM-DD\"T\"HH24:MI:SS.USOF'),s.payload_erased \
         FROM knowledge_maintenance_tasks t JOIN knowledge_maintenance_signals s \
          ON s.tenant_id=t.tenant_id AND s.workspace_id=t.workspace_id AND s.id=t.signal_id \
         WHERE t.tenant_id=$1 AND t.workspace_id=$2 AND ($3::uuid IS NULL OR s.unit_id=$3) \
          AND (cardinality($4::text[])=0 OR t.state=ANY($4)) \
          AND ($5::uuid IS NULL OR t.id>$5) AND NOT t.payload_erased AND NOT s.payload_erased \
         ORDER BY t.id LIMIT $6",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(query.unit_id)
    .bind(&states)
    .bind(query.after)
    .bind(i64::from(query.limit) + 1)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    let mut tasks = rows
        .into_iter()
        .map(task_from_row)
        .collect::<Result<Vec<_>>>()?;
    let next_after = if tasks.len() > query.limit as usize {
        tasks.pop();
        tasks.last().map(|task| task.id)
    } else {
        None
    };
    Ok(KnowledgeMaintenanceContext {
        workspace_generation: generation,
        method: method.clone(),
        tasks,
        next_after,
    })
}

pub(super) async fn load_task(
    connection: &mut PgConnection,
    tenant: Uuid,
    workspace: Uuid,
    task: Uuid,
) -> Result<KnowledgeMaintenanceTask> {
    let row = sqlx::query(
        "SELECT t.id,t.revision,t.state,t.attempts,t.failure_code, \
         to_char(t.next_retry_at,'YYYY-MM-DD\"T\"HH24:MI:SS.USOF'),t.change_id,t.run_id, \
         t.current_review,t.affected_consumers,t.terminal_evidence,t.payload_erased, \
         s.id,s.unit_id,s.unit_revision,s.reason,s.basis,s.basis_digest, \
         to_char(s.observed_at,'YYYY-MM-DD\"T\"HH24:MI:SS.USOF'),s.payload_erased \
         FROM knowledge_maintenance_tasks t JOIN knowledge_maintenance_signals s \
          ON s.tenant_id=t.tenant_id AND s.workspace_id=t.workspace_id AND s.id=t.signal_id \
         WHERE t.tenant_id=$1 AND t.workspace_id=$2 AND t.id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(task)
    .fetch_optional(connection)
    .await
    .map_err(storage_error)?
    .ok_or(Error::NotFound)?;
    task_from_row(row)
}

pub(crate) async fn linked_tasks(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    run: Uuid,
) -> Result<Vec<KnowledgeMaintenanceTask>> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM knowledge_maintenance_tasks WHERE tenant_id=$1 AND workspace_id=$2 \
         AND change_id=$3 AND run_id=$4 AND NOT payload_erased ORDER BY id LIMIT 65",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(change)
    .bind(run)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    if ids.len() > KNOWLEDGE_MAINTENANCE_MAX_BATCH as usize {
        return Err(Error::NeedsContext);
    }
    let mut tasks = Vec::with_capacity(ids.len());
    for id in ids {
        tasks.push(load_task(tx, tenant, workspace, id).await?);
    }
    Ok(tasks)
}

fn task_from_row(row: PgRow) -> Result<KnowledgeMaintenanceTask> {
    if row.get::<bool, _>(11) || row.get::<bool, _>(19) {
        return Err(Error::KnowledgePayloadErased);
    }
    let basis = decode(row.get::<serde_json::Value, _>(16))?;
    let reason = decode(serde_json::Value::String(row.get(15)))?;
    if basis_reason(&basis) != reason {
        return Err(Error::InternalInvariant);
    }
    Ok(KnowledgeMaintenanceTask {
        id: row.get(0),
        revision: row.get(1),
        state: decode(serde_json::Value::String(row.get(2)))?,
        attempts: u32::try_from(row.get::<i32, _>(3)).map_err(storage_error)?,
        failure_code: row
            .get::<Option<String>, _>(4)
            .map(|value| decode(serde_json::Value::String(value)))
            .transpose()?,
        next_retry_at: row.get(5),
        change_id: row.get(6),
        run_id: row.get(7),
        current_review: row
            .get::<Option<serde_json::Value>, _>(8)
            .map(decode)
            .transpose()?,
        affected_consumers: row
            .get::<Option<serde_json::Value>, _>(9)
            .map(decode)
            .transpose()?
            .unwrap_or_default(),
        terminal_evidence: row
            .get::<Option<serde_json::Value>, _>(10)
            .map(decode)
            .transpose()?,
        signal: KnowledgeMaintenanceSignal {
            id: row.get(12),
            unit_id: row.get(13),
            unit_revision: row.get(14),
            reason,
            basis,
            basis_digest: row.get(17),
            observed_at: row.get(18),
        },
    })
}

fn basis_reason(value: &KnowledgeMaintenanceBasis) -> KnowledgeMaintenanceSignalReason {
    value.reason()
}

fn enum_text<T: Serialize>(value: &T) -> Result<String> {
    match json(value)? {
        serde_json::Value::String(value) => Ok(value),
        _ => Err(Error::InternalInvariant),
    }
}
