use super::*;

struct TargetAssignment {
    operation_id: Uuid,
    operation: KnowledgeLifecycleOperation,
    unit_id: Uuid,
    expected_revision: Option<i64>,
    expected_lifecycle: Option<KnowledgeLifecycleState>,
}

async fn target_assignment(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    operation: Uuid,
) -> Result<TargetAssignment> {
    let row: Option<(String, Uuid, Option<i64>, Option<String>)> = sqlx::query_as(
        "SELECT operation,unit_id,expected_revision,expected_lifecycle \
         FROM knowledge_change_operations \
         WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3 AND id=$4 \
           AND NOT payload_erased",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(change)
    .bind(operation)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let (kind, unit_id, expected_revision, expected_lifecycle) =
        row.ok_or(Error::InvalidArguments)?;
    Ok(TargetAssignment {
        operation_id: operation,
        operation: decode(serde_json::Value::String(kind))?,
        unit_id,
        expected_revision,
        expected_lifecycle: expected_lifecycle
            .map(|value| decode(serde_json::Value::String(value)))
            .transpose()?,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn apply(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    amendment: &KnowledgeBasisAmendment,
) -> Result<KnowledgeAppliedBasisAmendment> {
    let mut assignments = Vec::with_capacity(amendment.target_updates.len());
    for update in &amendment.target_updates {
        let assignment =
            target_assignment(tx, tenant, workspace, change, update.operation_id).await?;
        if assignment.operation == KnowledgeLifecycleOperation::Create
            || assignment.unit_id != update.replacement_guard.unit_id
            || assignment.expected_revision != Some(update.previous_expected_revision)
            || assignment.expected_lifecycle != Some(update.previous_expected_lifecycle)
        {
            return Err(Error::ContextChanged);
        }
        if update.replacement_guard.revision == update.previous_expected_revision
            && update.replacement_guard.lifecycle == update.previous_expected_lifecycle
        {
            return Err(Error::InvalidArguments);
        }
        super::event::verify_revision_guard(tx, tenant, workspace, &update.replacement_guard)
            .await?;
        assignments.push((assignment, update));
    }

    let source_change = if let Some(replacement_sources) = &amendment.replacement_sources {
        let row: (serde_json::Value, serde_json::Value, i64) = sqlx::query_as(
            "SELECT sources,source_pins,source_revision FROM knowledge_lifecycle_changes \
             WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND NOT payload_erased",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(change)
        .fetch_optional(&mut **tx)
        .await
        .map_err(storage_error)?
        .ok_or(Error::KnowledgePayloadErased)?;
        let previous_sources: Vec<KnowledgeSourceRef> = decode(row.0)?;
        let previous_pins: Vec<KnowledgeResolvedSourcePin> = decode(row.1)?;
        if previous_sources == *replacement_sources {
            return Err(Error::InvalidArguments);
        }
        let replacement_source_revision = row.2.checked_add(1).ok_or(Error::CapacityExceeded)?;
        let replacement_pins = super::phase_data::resolve_sources_at_revision(
            tx,
            tenant,
            workspace,
            change,
            replacement_source_revision,
            replacement_sources,
        )
        .await?
        .into_iter()
        .map(|value| value.pin)
        .collect::<Vec<_>>();
        Some(KnowledgeSourceBasisChange {
            previous_sources,
            previous_pins,
            previous_source_revision: row.2,
            replacement_sources: replacement_sources.clone(),
            replacement_pins,
            replacement_source_revision,
        })
    } else {
        None
    };

    if assignments.is_empty() && source_change.is_none() {
        return Err(Error::InvalidArguments);
    }
    for (assignment, update) in assignments {
        sqlx::query(
            "UPDATE knowledge_change_operations \
             SET expected_revision=$5,expected_lifecycle=$6 \
             WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3 AND id=$4",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(change)
        .bind(assignment.operation_id)
        .bind(update.replacement_guard.revision)
        .bind(enum_text(&update.replacement_guard.lifecycle)?)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    }
    if let Some(source_change) = &source_change {
        sqlx::query(
            "UPDATE knowledge_lifecycle_changes \
             SET sources=$4,source_pins=$5,source_revision=$6,updated_at=pg_catalog.clock_timestamp() \
             WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(change)
        .bind(json(&source_change.replacement_sources)?)
        .bind(json(&source_change.replacement_pins)?)
        .bind(source_change.replacement_source_revision)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    }
    Ok(KnowledgeAppliedBasisAmendment {
        target_updates: amendment.target_updates.clone(),
        source_change,
    })
}
