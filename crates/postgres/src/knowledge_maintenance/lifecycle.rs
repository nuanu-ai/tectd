use super::*;

const MAX_DEPENDENTS_PER_OPERATION: usize = 512;

pub(crate) async fn publication_applied(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    receipt: &KnowledgePublisherReceipt,
) -> Result<()> {
    resolve_linked_tasks(tx, tenant, workspace, receipt).await?;
    for applied in &receipt.applied_operations {
        create_dependency_signals(tx, tenant, workspace, principal, applied).await?;
    }
    Ok(())
}

pub(crate) async fn reset_restored_leases(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    sqlx::query(
        "UPDATE knowledge_maintenance_tasks SET state=CASE WHEN attempts>=5 THEN 'exhausted' \
         ELSE 'pending' END,next_retry_at=CASE WHEN attempts>=5 THEN NULL \
         ELSE pg_catalog.clock_timestamp() END,failure_code='lease_expired',lease_token=NULL,lease_expires_at=NULL, \
         revision=revision+1,updated_at=pg_catalog.clock_timestamp() WHERE state='leased'",
    )
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(())
}

async fn resolve_linked_tasks(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    receipt: &KnowledgePublisherReceipt,
) -> Result<()> {
    let rows: Vec<(Uuid, Uuid, i64, serde_json::Value, String, Uuid)> = sqlx::query_as(
        "SELECT t.id,s.unit_id,s.unit_revision,s.basis,s.basis_digest,t.run_id \
         FROM knowledge_maintenance_tasks t \
         JOIN knowledge_maintenance_signals s ON s.tenant_id=t.tenant_id \
          AND s.workspace_id=t.workspace_id AND s.id=t.signal_id \
         WHERE t.tenant_id=$1 AND t.workspace_id=$2 AND t.change_id=$3 \
          AND t.state='linked' AND NOT t.payload_erased AND NOT s.payload_erased FOR UPDATE OF t",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(receipt.change_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    for (task, unit, signal_revision, basis_value, basis_digest, linked_run) in rows {
        if linked_run != receipt.run_id {
            return Err(Error::InternalInvariant);
        }
        let review =
            crate::knowledge_lifecycle::review_receipt(tx, tenant, workspace, linked_run).await?;
        if review.outcome != KnowledgeReviewOutcome::Ready
            || !review.reviewed_digests.contains(&basis_digest)
        {
            return Err(Error::NeedsContext);
        }
        let applied = receipt
            .applied_operations
            .iter()
            .find(|operation| operation.unit_id == unit)
            .ok_or(Error::InternalInvariant)?;
        if matches!(
            applied.operation,
            KnowledgeLifecycleOperation::Retract | KnowledgeLifecycleOperation::Erase
        ) {
            obsolete(tx, tenant, workspace, task).await?;
            continue;
        }
        let revision = applied.revision.ok_or(Error::InternalInvariant)?;
        if !matches!(
            applied.operation,
            KnowledgeLifecycleOperation::Revise
                | KnowledgeLifecycleOperation::Revalidate
                | KnowledgeLifecycleOperation::Supersede
        ) {
            return Err(Error::InternalInvariant);
        }
        let verified = crate::knowledge_lifecycle::verify_publication_event(
            tx,
            tenant,
            workspace,
            unit,
            revision,
            applied.event_id,
            applied.operation == KnowledgeLifecycleOperation::Revise,
        )
        .await?;
        if verified.input.change_id != receipt.change_id
            || verified.input.planned.operation_id != applied.operation_id
        {
            return Err(Error::InternalInvariant);
        }
        let basis: KnowledgeMaintenanceBasis = decode(basis_value)?;
        if let KnowledgeMaintenanceBasis::SourceChanged {
            source_iri,
            accepted_digest,
            observed_digest,
        } = &basis
        {
            let logical_uri = super::source::verified_logical_uri(
                tx,
                tenant,
                workspace,
                unit,
                signal_revision,
                source_iri,
                accepted_digest,
            )
            .await?
            .ok_or(Error::NeedsContext)?;
            if !verified
                .input
                .resolved_sources
                .iter()
                .any(|source| source.uri == logical_uri && source.pin.digest == *observed_digest)
            {
                return Err(Error::NeedsContext);
            }
        }
        let validation_event_id = (applied.operation == KnowledgeLifecycleOperation::Revalidate)
            .then_some(applied.event_id);
        if applied.operation == KnowledgeLifecycleOperation::Revalidate {
            let exact: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM knowledge_validation_events WHERE tenant_id=$1 \
                 AND workspace_id=$2 AND id=$3 AND unit_id=$4 AND unit_revision=$5 \
                 AND lifecycle_change_id=$6 AND operation_id=$7 AND NOT payload_erased)",
            )
            .bind(tenant)
            .bind(workspace)
            .bind(applied.event_id)
            .bind(unit)
            .bind(signal_revision)
            .bind(receipt.change_id)
            .bind(applied.operation_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(storage_error)?;
            if !exact {
                return Err(Error::InternalInvariant);
            }
        }
        let evidence = KnowledgeMaintenanceTerminalEvidence {
            change_id: receipt.change_id,
            publisher_receipt_id: receipt.id,
            operation_id: applied.operation_id,
            event_id: applied.event_id,
            operation: applied.operation,
            unit_revision: revision,
            validation_event_id,
        };
        sqlx::query(
            "UPDATE knowledge_maintenance_tasks SET state='resolved',revision=revision+1, \
             terminal_evidence=$4,updated_at=pg_catalog.clock_timestamp() \
             WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state='linked'",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(task)
        .bind(json(&evidence)?)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    }
    Ok(())
}

async fn create_dependency_signals(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    applied: &KnowledgeAppliedOperationReceipt,
) -> Result<()> {
    let previous: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM (SELECT id,created_at FROM knowledge_publication_events \
          WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND id<>$4 \
          UNION ALL SELECT id,created_at FROM knowledge_validation_events \
          WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND id<>$4) events \
         ORDER BY created_at DESC,id DESC LIMIT 1",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(applied.unit_id)
    .bind(applied.event_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let dependents: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT h.unit_id,h.accepted_revision FROM knowledge_unit_heads h \
         JOIN knowledge_revisions r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id \
          AND r.unit_id=h.unit_id AND r.revision=h.accepted_revision \
         WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.unit_id<>$3 \
          AND h.contract_version='dk-2' AND h.lifecycle='active' AND h.active \
          AND NOT h.payload_erased AND NOT r.payload_erased \
          AND COALESCE(r.document_payload#>'{sections,runbook,dependency_iris}','[]'::jsonb) ? $4 \
         ORDER BY h.unit_id LIMIT 513",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(applied.unit_id)
    .bind(&applied.unit_iri)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    if dependents.len() > MAX_DEPENDENTS_PER_OPERATION {
        return Err(Error::CapacityExceeded);
    }
    for (unit, revision) in dependents {
        current_unit_review_status(tx, tenant, workspace, principal, unit, revision).await?;
        let _ = super::signal::ensure_signal_task(
            tx,
            tenant,
            workspace,
            principal,
            None,
            unit,
            revision,
            &KnowledgeMaintenanceBasis::DependencyChanged {
                dependency_unit_id: applied.unit_id,
                previous_event_id: previous,
                observed_event_id: applied.event_id,
            },
            None,
            false,
        )
        .await?;
    }
    Ok(())
}

async fn obsolete(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    task: Uuid,
) -> Result<()> {
    sqlx::query(
        "UPDATE knowledge_maintenance_tasks SET state='obsolete',revision=revision+1, \
         lease_token=NULL,lease_expires_at=NULL,updated_at=pg_catalog.clock_timestamp() \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(task)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(())
}
