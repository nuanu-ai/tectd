use super::*;

type StatusRow = (i64, String, String, String, Uuid, bool, bool);

pub(crate) async fn current_unit_review_status(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    unit: Uuid,
    revision: i64,
) -> Result<KnowledgeUnitReviewStatus> {
    require_identity(tx).await?;
    let row: Option<StatusRow> = sqlx::query_as(
        "SELECT h.accepted_revision,h.lifecycle,h.access_scope,r.access_scope, \
         r.publication_event_id,h.payload_erased,r.payload_erased \
         FROM knowledge_unit_heads h JOIN knowledge_revisions r \
           ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id \
          AND r.unit_id=h.unit_id AND r.revision=$4 \
         WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.unit_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(unit)
    .bind(revision)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some((_head_revision, lifecycle, head_access, revision_access, event, head_erased, erased)) =
        row
    else {
        return Err(Error::NotFound);
    };
    if head_access == "owners_only" || revision_access == "owners_only" {
        require_owner(tx, principal).await?;
    }
    if head_erased || erased || matches!(lifecycle.as_str(), "erased" | "erasure_pending") {
        return Err(Error::KnowledgePayloadErased);
    }
    let response = crate::knowledge_lifecycle::unit(
        tx,
        tenant,
        workspace,
        principal,
        &KnowledgeUnitQuery {
            unit_id: unit,
            revision: Some(revision),
            fragment: None,
        },
    )
    .await?
    .ok_or(Error::NotFound)?;
    let (valid_from, mut valid_until, mut review_due_at, access_scope) = match response {
        KnowledgeUnitResponse::Document(value) => (
            value.document.valid_from,
            value.document.valid_until,
            value.document.review_due_at,
            value.document.access_scope,
        ),
        KnowledgeUnitResponse::LegacyConstraint(_) => {
            (None, None, None, parse_access(&revision_access)?)
        }
        KnowledgeUnitResponse::PayloadErased(_) => return Err(Error::KnowledgePayloadErased),
    };
    let validation_event: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM knowledge_validation_events WHERE tenant_id=$1 AND workspace_id=$2 \
         AND unit_id=$3 AND unit_revision=$4 AND NOT payload_erased \
         ORDER BY created_at DESC,id DESC LIMIT 1",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(unit)
    .bind(revision)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if let Some(validation) = validation_event {
        let verified = crate::knowledge_lifecycle::verify_publication_event(
            tx, tenant, workspace, unit, revision, validation, false,
        )
        .await?;
        if verified.input.planned.operation != KnowledgeLifecycleOperation::Revalidate {
            return Err(Error::InternalInvariant);
        }
        let value = verified
            .input
            .planned
            .revalidation
            .ok_or(Error::InternalInvariant)?;
        valid_until = value.valid_until.or(valid_until);
        review_due_at = value.review_due_at.or(review_due_at);
    }
    let (due, not_yet_valid, expired): (bool, bool, bool) = sqlx::query_as(
        "SELECT ($1::timestamptz IS NOT NULL AND $1::timestamptz<=pg_catalog.clock_timestamp()), \
                ($2::timestamptz IS NOT NULL AND $2::timestamptz>pg_catalog.clock_timestamp()), \
                ($3::timestamptz IS NOT NULL AND $3::timestamptz<pg_catalog.clock_timestamp())",
    )
    .bind(review_due_at.as_deref())
    .bind(valid_from.as_deref())
    .bind(valid_until.as_deref())
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let bases: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT t.id,s.reason,s.basis_digest FROM knowledge_maintenance_tasks t \
         JOIN knowledge_maintenance_signals s ON s.tenant_id=t.tenant_id \
          AND s.workspace_id=t.workspace_id AND s.id=t.signal_id \
         WHERE t.tenant_id=$1 AND t.workspace_id=$2 AND s.unit_id=$3 \
          AND s.unit_revision=$4 AND NOT t.payload_erased AND NOT s.payload_erased \
          AND (t.state IN ('pending','leased','needs_review','linked','exhausted') \
           OR (t.state='resolved' AND COALESCE((t.terminal_evidence->>'unit_revision')::bigint,-1) \
            <> s.unit_revision)) \
         ORDER BY s.observed_at,t.id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(unit)
    .bind(revision)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    let maintenance_bases = bases
        .into_iter()
        .map(|(task_id, reason, basis_digest)| {
            Ok(KnowledgeMaintenanceReviewBasis {
                task_id,
                reason: decode(serde_json::Value::String(reason))?,
                basis_digest,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(KnowledgeUnitReviewStatus {
        unit_id: unit,
        revision,
        lifecycle: decode(serde_json::Value::String(lifecycle))?,
        access_scope,
        publication_event_id: event,
        validation_event_id: validation_event,
        valid_from,
        valid_until,
        review_due_at,
        due,
        not_yet_valid,
        expired,
        needs_review: due || expired || !maintenance_bases.is_empty(),
        maintenance_bases,
    })
}

fn parse_access(value: &str) -> Result<KnowledgeAccessScope> {
    decode(serde_json::Value::String(value.into()))
}
