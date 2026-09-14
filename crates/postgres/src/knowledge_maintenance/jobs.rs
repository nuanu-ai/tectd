use super::*;

pub(crate) async fn sweep_due(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    limit: u32,
) -> Result<u32> {
    require_identity(tx).await?;
    require_owner(tx, principal).await?;
    publisher_lock(tx).await?;
    crate::durable_knowledge::lock_state(tx, tenant, workspace).await?;
    let rows: Vec<(Uuid, i64)> = sqlx::query_as(
        "WITH due_units AS (SELECT h.unit_id,h.accepted_revision, \
          NULLIF(r.document_payload->>'valid_from','')::timestamptz valid_from, \
          COALESCE(v.review_due_at,NULLIF(r.document_payload->>'review_due_at','')::timestamptz) due_at, \
          COALESCE(v.valid_until,NULLIF(r.document_payload->>'valid_until','')::timestamptz) valid_until, \
          v.id validation_event_id FROM knowledge_unit_heads h \
          JOIN knowledge_revisions r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id \
           AND r.unit_id=h.unit_id AND r.revision=h.accepted_revision \
          LEFT JOIN LATERAL (SELECT e.id,e.review_due_at,e.valid_until FROM knowledge_validation_events e \
           WHERE e.tenant_id=h.tenant_id AND e.workspace_id=h.workspace_id \
            AND e.unit_id=h.unit_id AND e.unit_revision=h.accepted_revision AND NOT e.payload_erased \
           ORDER BY e.created_at DESC,e.id DESC LIMIT 1) v ON true \
          WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.contract_version='dk-2' \
           AND h.lifecycle='active' AND h.active AND NOT h.payload_erased AND NOT r.payload_erased) \
         SELECT d.unit_id,d.accepted_revision FROM due_units d \
          WHERE (d.valid_from IS NULL OR d.valid_from<=pg_catalog.clock_timestamp()) \
           AND (d.due_at<=pg_catalog.clock_timestamp() OR d.valid_until<pg_catalog.clock_timestamp()) \
           AND NOT EXISTS(SELECT 1 \
           FROM knowledge_maintenance_signals s WHERE s.tenant_id=$1 AND s.workspace_id=$2 \
            AND s.unit_id=d.unit_id AND s.unit_revision=d.accepted_revision \
            AND s.reason='review_due' AND NOT s.payload_erased \
            AND NULLIF(s.basis->>'review_due_at','')::timestamptz IS NOT DISTINCT FROM d.due_at \
            AND NULLIF(s.basis->>'valid_until','')::timestamptz IS NOT DISTINCT FROM d.valid_until \
            AND COALESCE(s.basis->>'validation_event_id','')=COALESCE(d.validation_event_id::text,'')) \
         ORDER BY d.unit_id LIMIT $3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(i64::from(limit))
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    let mut created = 0;
    for (unit, revision) in rows {
        let status =
            current_unit_review_status(tx, tenant, workspace, principal, unit, revision).await?;
        if status.not_yet_valid || !status.due && !status.expired {
            return Err(Error::InternalInvariant);
        }
        let (_, inserted) = super::signal::ensure_signal_task(
            tx,
            tenant,
            workspace,
            principal,
            None,
            unit,
            revision,
            &KnowledgeMaintenanceBasis::ReviewDue {
                review_due_at: status.review_due_at,
                valid_until: status.valid_until,
                validation_event_id: status.validation_event_id,
            },
            None,
            true,
        )
        .await?;
        if !inserted {
            return Err(Error::InternalInvariant);
        }
        created += 1;
    }
    Ok(created)
}

pub(crate) async fn claim(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
) -> Result<KnowledgeMaintenanceClaimOutcome> {
    require_identity(tx).await?;
    require_owner(tx, principal).await?;
    publisher_lock(tx).await?;
    crate::durable_knowledge::lock_state(tx, tenant, workspace).await?;
    let exhausted: i64 = sqlx::query_scalar(
        "WITH recovered AS (UPDATE knowledge_maintenance_tasks SET \
         state=CASE WHEN attempts>=5 THEN 'exhausted' ELSE 'pending' END, \
         next_retry_at=CASE WHEN attempts>=5 THEN NULL ELSE pg_catalog.clock_timestamp() \
           + pg_catalog.make_interval(secs=>LEAST(attempts*60,300)) END, \
         failure_code='lease_expired',lease_token=NULL,lease_expires_at=NULL,revision=revision+1, \
         updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 \
         AND state='leased' AND lease_expires_at<=pg_catalog.clock_timestamp() \
         RETURNING state) SELECT count(*) FILTER (WHERE state='exhausted') FROM recovered",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let row: Option<(Uuid, i64, Uuid)> = sqlx::query_as(
        "WITH candidate AS (SELECT t.id FROM knowledge_maintenance_tasks t \
          JOIN knowledge_maintenance_signals s ON s.tenant_id=t.tenant_id \
           AND s.workspace_id=t.workspace_id AND s.id=t.signal_id \
          JOIN knowledge_revisions r ON r.tenant_id=s.tenant_id AND r.workspace_id=s.workspace_id \
           AND r.unit_id=s.unit_id AND r.revision=s.unit_revision \
          WHERE t.tenant_id=$1 AND t.workspace_id=$2 AND t.state='pending' AND t.attempts<5 \
           AND NOT t.payload_erased AND NOT s.payload_erased AND NOT r.payload_erased \
           AND (NULLIF(r.document_payload->>'valid_from','')::timestamptz IS NULL \
            OR NULLIF(r.document_payload->>'valid_from','')::timestamptz<=pg_catalog.clock_timestamp()) \
           AND COALESCE(t.next_retry_at,'-infinity'::timestamptz)<=pg_catalog.clock_timestamp() \
          ORDER BY COALESCE(t.next_retry_at,t.created_at),t.id LIMIT 1 FOR UPDATE OF t SKIP LOCKED) \
         UPDATE knowledge_maintenance_tasks t SET state='leased',attempts=attempts+1, \
          lease_token=pg_catalog.gen_random_uuid(),lease_expires_at=pg_catalog.clock_timestamp() \
            + pg_catalog.make_interval(secs=>60),next_retry_at=NULL,revision=revision+1, \
          updated_at=pg_catalog.clock_timestamp() FROM candidate c WHERE t.id=c.id \
         RETURNING t.id,t.revision,t.lease_token",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(KnowledgeMaintenanceClaimOutcome {
        claim: row.map(
            |(task_id, task_revision, lease_token)| KnowledgeMaintenanceJobClaim {
                task_id,
                task_revision,
                lease_token,
                workspace_id: workspace,
                principal_id: principal,
            },
        ),
        exhausted: u32::try_from(exhausted).map_err(storage_error)?,
    })
}

pub(crate) async fn prepare(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    claim: &KnowledgeMaintenanceJobClaim,
) -> Result<KnowledgeMaintenancePrepareOutcome> {
    require_identity(tx).await?;
    require_owner(tx, principal).await?;
    if claim.workspace_id != workspace || claim.principal_id != principal {
        return Err(Error::Forbidden);
    }
    publisher_lock(tx).await?;
    crate::durable_knowledge::lock_state(tx, tenant, workspace).await?;
    let row: Option<(i64, String, Option<Uuid>, bool, i32, Uuid)> = sqlx::query_as(
        "SELECT revision,state,lease_token,COALESCE(lease_expires_at>pg_catalog.clock_timestamp(),false),attempts,signal_id \
         FROM knowledge_maintenance_tasks WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 \
         FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(claim.task_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some((revision, state, lease, lease_live, attempts, signal_id)) = row else {
        return Err(Error::NotFound);
    };
    if revision != claim.task_revision
        || state != "leased"
        || lease != Some(claim.lease_token)
        || !lease_live
    {
        return Err(Error::ContextChanged);
    }
    let signal: (Uuid, i64, serde_json::Value) = sqlx::query_as(
        "SELECT unit_id,unit_revision,basis FROM knowledge_maintenance_signals \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND NOT payload_erased",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(signal_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?
    .ok_or(Error::KnowledgePayloadErased)?;
    let basis: KnowledgeMaintenanceBasis = decode(signal.2)?;
    let current_revision: Option<i64> = sqlx::query_scalar(
        "SELECT accepted_revision FROM knowledge_unit_heads WHERE tenant_id=$1 \
         AND workspace_id=$2 AND unit_id=$3 AND lifecycle='active' AND active \
         AND NOT payload_erased",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(signal.0)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    super::reconcile_unit_consumers(tx, tenant, workspace, signal.0).await?;
    let consumers = registered_consumers(tx, tenant, workspace, signal.0, signal.1).await?;
    let retained_consumed = !consumers.is_empty();
    let current = if current_revision == Some(signal.1) || retained_consumed {
        match current_unit_review_status(tx, tenant, workspace, principal, signal.0, signal.1).await
        {
            Ok(value) => Some(value),
            Err(Error::NotFound | Error::KnowledgePayloadErased) => None,
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    let consumers = if current.is_some() {
        consumers
    } else {
        Vec::new()
    };
    let applicable = if let Some(status) = &current {
        basis_applies(
            tx, tenant, workspace, signal.0, signal.1, &basis, status, &consumers,
        )
        .await?
    } else {
        false
    };
    let outcome = if applicable {
        sqlx::query(
            "UPDATE knowledge_maintenance_tasks SET state='needs_review',revision=revision+1, \
             failure_code=NULL,lease_token=NULL,lease_expires_at=NULL,current_review=$4,affected_consumers=$5, \
             updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(claim.task_id)
        .bind(json(current.as_ref().ok_or(Error::InternalInvariant)?)?)
        .bind(json(&consumers)?)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
        KnowledgeMaintenancePrepareOutcome::NeedsReview
    } else {
        sqlx::query(
            "UPDATE knowledge_maintenance_tasks SET state='obsolete',revision=revision+1, \
             failure_code=NULL,lease_token=NULL,lease_expires_at=NULL,updated_at=pg_catalog.clock_timestamp() \
             WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(claim.task_id)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
        KnowledgeMaintenancePrepareOutcome::Obsolete
    };
    if attempts >= 5 && !applicable {
        return Ok(outcome);
    }
    Ok(outcome)
}

pub(crate) async fn fail(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    claim: &KnowledgeMaintenanceJobClaim,
    code: KnowledgeMaintenanceFailureCode,
) -> Result<KnowledgeMaintenanceFailureOutcome> {
    require_identity(tx).await?;
    require_owner(tx, principal).await?;
    if claim.workspace_id != workspace || claim.principal_id != principal {
        return Err(Error::Forbidden);
    }
    publisher_lock(tx).await?;
    crate::durable_knowledge::lock_state(tx, tenant, workspace).await?;
    let row: Option<(i64, String, Option<Uuid>, bool, i32)> = sqlx::query_as(
        "SELECT revision,state,lease_token,COALESCE(lease_expires_at>pg_catalog.clock_timestamp(),false),attempts \
         FROM knowledge_maintenance_tasks WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 \
         FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(claim.task_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some((revision, state, lease, lease_live, attempts)) = row else {
        return Err(Error::NotFound);
    };
    if revision != claim.task_revision || state != "leased" || lease != Some(claim.lease_token) {
        return Err(Error::ContextChanged);
    }
    if code == KnowledgeMaintenanceFailureCode::LeaseExpired {
        if lease_live {
            return Err(Error::ContextChanged);
        }
    } else if !lease_live {
        return Err(Error::ContextChanged);
    }
    let outcome = if code == KnowledgeMaintenanceFailureCode::NeedsContext {
        KnowledgeMaintenanceFailureOutcome::NeedsReview
    } else if attempts >= KNOWLEDGE_MAINTENANCE_MAX_ATTEMPTS as i32 {
        KnowledgeMaintenanceFailureOutcome::Exhausted
    } else {
        KnowledgeMaintenanceFailureOutcome::RetryScheduled
    };
    let state = match outcome {
        KnowledgeMaintenanceFailureOutcome::RetryScheduled => "pending",
        KnowledgeMaintenanceFailureOutcome::NeedsReview => "needs_review",
        KnowledgeMaintenanceFailureOutcome::Exhausted => "exhausted",
    };
    let delay = i64::from(attempts).saturating_mul(60).min(300);
    sqlx::query(
        "UPDATE knowledge_maintenance_tasks SET state=$4,failure_code=$5, \
         next_retry_at=CASE WHEN $4='pending' THEN pg_catalog.clock_timestamp() \
          + pg_catalog.make_interval(secs=>$6) ELSE NULL END,lease_token=NULL,lease_expires_at=NULL, \
         revision=revision+1,updated_at=pg_catalog.clock_timestamp() \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(claim.task_id)
    .bind(state)
    .bind(match json(&code)? {
        serde_json::Value::String(value) => value,
        _ => return Err(Error::InternalInvariant),
    })
    .bind(delay)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(outcome)
}

pub(crate) async fn pending(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
) -> Result<u32> {
    require_identity(tx).await?;
    require_owner(tx, principal).await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM knowledge_maintenance_tasks WHERE tenant_id=$1 AND workspace_id=$2 \
         AND state IN ('pending','leased') AND NOT payload_erased",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    u32::try_from(count).map_err(storage_error)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn basis_applies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: i64,
    basis: &KnowledgeMaintenanceBasis,
    status: &KnowledgeUnitReviewStatus,
    consumers: &[KnowledgeMaintenanceConsumer],
) -> Result<bool> {
    match basis {
        KnowledgeMaintenanceBasis::ReviewDue {
            review_due_at,
            valid_until,
            validation_event_id,
        } => Ok((status.due || status.expired)
            && status.review_due_at == *review_due_at
            && status.valid_until == *valid_until
            && status.validation_event_id == *validation_event_id),
        KnowledgeMaintenanceBasis::SourceChanged {
            source_iri,
            accepted_digest,
            observed_digest,
        } => Ok(accepted_digest != observed_digest
            && super::source::verified_logical_uri(
                tx,
                tenant,
                workspace,
                unit,
                revision,
                source_iri,
                accepted_digest,
            )
            .await?
            .is_some()),
        KnowledgeMaintenanceBasis::DependencyChanged {
            dependency_unit_id,
            observed_event_id,
            ..
        } => {
            let row: Option<String> = sqlx::query_scalar(
                "SELECT r.unit_iri FROM knowledge_unit_heads h JOIN knowledge_revisions r \
                  ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id \
                   AND r.unit_id=h.unit_id AND r.revision=h.accepted_revision \
                 WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.unit_id=$3 AND (EXISTS(SELECT 1 \
                  FROM knowledge_publication_events e WHERE e.tenant_id=h.tenant_id \
                   AND e.workspace_id=h.workspace_id AND e.unit_id=h.unit_id AND e.id=$4) \
                  OR EXISTS(SELECT 1 FROM knowledge_validation_events e \
                   WHERE e.tenant_id=h.tenant_id AND e.workspace_id=h.workspace_id \
                    AND e.unit_id=h.unit_id AND e.id=$4))",
            )
            .bind(tenant)
            .bind(workspace)
            .bind(dependency_unit_id)
            .bind(observed_event_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(storage_error)?;
            let Some(iri) = row else {
                return Ok(false);
            };
            let document: serde_json::Value = sqlx::query_scalar(
                "SELECT document_payload FROM knowledge_revisions WHERE tenant_id=$1 \
                 AND workspace_id=$2 AND unit_id=$3 AND revision=$4 AND NOT payload_erased",
            )
            .bind(tenant)
            .bind(workspace)
            .bind(unit)
            .bind(revision)
            .fetch_one(&mut **tx)
            .await
            .map_err(storage_error)?;
            let value: KnowledgeDocumentDraft = decode(document)?;
            Ok(value
                .sections
                .runbook
                .as_ref()
                .is_some_and(|runbook| runbook.dependency_iris.contains(&iri)))
        }
        KnowledgeMaintenanceBasis::ApplicationFailed { consumer_ref, .. } => Ok(consumers
            .iter()
            .any(|value| value.consumer_ref == *consumer_ref)),
        KnowledgeMaintenanceBasis::OperatorRequested { .. } => Ok(true),
    }
}
