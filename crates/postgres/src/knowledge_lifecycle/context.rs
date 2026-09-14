use super::*;

type LifecycleOutputRow = (
    String,
    Option<serde_json::Value>,
    bool,
    i64,
    Uuid,
    bool,
    Option<String>,
);
type UnitRevisionRow = (
    String,
    String,
    String,
    i64,
    String,
    bool,
    Option<serde_json::Value>,
    Option<String>,
    String,
    String,
    Uuid,
);

mod load;
pub(crate) use load::load_context;

pub(crate) async fn lifecycle(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    query: &KnowledgeLifecycleQuery,
) -> Result<KnowledgeLifecycleResponse> {
    require_owner(tx, principal).await?;
    let generation: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT generation FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2),0)",
    ).bind(tenant).bind(workspace).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let Some(change_id) = query.change_id else {
        let rows: Vec<(Uuid,Uuid,String,Option<String>,i64)> = sqlx::query_as(
            "SELECT c.id,r.id,r.status,r.current_phase_id,(SELECT count(*) FROM knowledge_change_operations o WHERE o.tenant_id=c.tenant_id AND o.workspace_id=c.workspace_id AND o.change_id=c.id) \
             FROM knowledge_lifecycle_changes c JOIN knowledge_change_runs r ON r.tenant_id=c.tenant_id AND r.workspace_id=c.workspace_id AND r.change_id=c.id \
             WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND r.status IN ('active','waiting_input','blocked') ORDER BY c.created_at,c.id",
        ).bind(tenant).bind(workspace).fetch_all(&mut **tx).await.map_err(storage_error)?;
        let active = rows
            .into_iter()
            .map(|row| {
                Ok(KnowledgeLifecycleSummary {
                    change_id: row.0,
                    run_id: row.1,
                    status: run_status(&row.2)?,
                    current_phase_id: row.3.map(|value| phase(&value)).transpose()?,
                    operation_count: row.4 as u32,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok(KnowledgeLifecycleResponse::Overview(
            KnowledgeLifecycleOverview {
                workspace_generation: generation,
                active,
            },
        ));
    };
    match query.view {
        KnowledgeLifecycleView::Current => Ok(KnowledgeLifecycleResponse::Current(Box::new(
            load_context(tx, tenant, workspace, change_id)
                .await?
                .ok_or(Error::NotFound)?,
        ))),
        KnowledgeLifecycleView::History => {
            let context = load_context(tx, tenant, workspace, change_id)
                .await?
                .ok_or(Error::NotFound)?;
            Ok(KnowledgeLifecycleResponse::History(context.attempts))
        }
        KnowledgeLifecycleView::Output => {
            let output_id = query.output_id.ok_or(Error::InvalidArguments)?;
            let row:Option<LifecycleOutputRow>=sqlx::query_as(
                "SELECT digest,output,payload_erased,revision,run_id,COALESCE(b.stale,true),b.stale_reason FROM knowledge_change_outputs o LEFT JOIN knowledge_change_output_bindings b ON b.tenant_id=o.tenant_id AND b.workspace_id=o.workspace_id AND b.run_id=o.run_id AND b.output_id=o.id JOIN knowledge_change_runs r ON r.tenant_id=o.tenant_id AND r.workspace_id=o.workspace_id AND r.id=o.run_id WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND r.change_id=$3 AND o.id=$4"
            ).bind(tenant).bind(workspace).bind(change_id).bind(output_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
            let row = row.ok_or(Error::NotFound)?;
            if row.0 != query.digest.as_deref().ok_or(Error::InvalidArguments)? {
                return Err(Error::NotFound);
            }
            if row.2 {
                return Err(Error::KnowledgePayloadErased);
            }
            Ok(KnowledgeLifecycleResponse::Output(Box::new(
                KnowledgePhaseOutput {
                    id: output_id,
                    run_id: row.4,
                    revision: row.3,
                    digest: row.0,
                    output: decode(row.1.ok_or(Error::InternalInvariant)?)?,
                    stale: row.5,
                    stale_reason: row.6,
                },
            )))
        }
    }
}

pub(crate) async fn unit(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    query: &KnowledgeUnitQuery,
) -> Result<Option<KnowledgeUnitResponse>> {
    let row:Option<UnitRevisionRow>=sqlx::query_as(
        "SELECT h.lifecycle,h.access_scope,r.access_scope,r.revision,r.contract_version,r.payload_erased,r.document_payload,r.rdf_digest,r.unit_iri,r.revision_iri,r.publication_event_id FROM knowledge_unit_heads h JOIN knowledge_revisions r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id AND r.unit_id=h.unit_id AND r.revision=COALESCE($4,h.accepted_revision) WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.unit_id=$3"
    ).bind(tenant).bind(workspace).bind(query.unit_id).bind(query.revision).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((
        lifecycle,
        head_access,
        revision_access,
        revision,
        contract,
        erased,
        document,
        revision_rdf_digest,
        unit_iri,
        revision_iri,
        event,
    )) = row
    else {
        return Ok(None);
    };
    if head_access == "owners_only" || revision_access == "owners_only" {
        require_owner(tx, principal).await?;
    }
    if lifecycle == "erased" || lifecycle == "erasure_pending" || erased {
        return Ok(Some(KnowledgeUnitResponse::PayloadErased(
            KnowledgeUnitTombstone {
                unit_id: query.unit_id,
                lifecycle: decode(serde_json::Value::String(lifecycle))?,
            },
        )));
    }
    if contract == "dk-1" {
        return Ok(crate::durable_knowledge::context::load_revision(
            tx,
            tenant,
            workspace,
            query.unit_id,
            Some(revision),
            true,
        )
        .await?
        .map(|value| KnowledgeUnitResponse::LegacyConstraint(Box::new(value))));
    }
    let verified = event::verify_publication_event(
        tx,
        tenant,
        workspace,
        query.unit_id,
        revision,
        event,
        true,
    )
    .await?;
    let input = verified.input;
    let expected = rdf::build(&input)?;
    if expected.refs.unit != unit_iri || expected.refs.revision != revision_iri {
        return Err(Error::InternalInvariant);
    }
    let verified_document = input
        .planned
        .document
        .as_ref()
        .ok_or(Error::InternalInvariant)?;
    if document.as_ref() != Some(&json(verified_document)?) {
        return Err(Error::InternalInvariant);
    }
    if revision_rdf_digest.as_deref() != Some(verified.rdf_digest.as_str()) {
        return Err(Error::InternalInvariant);
    }
    let document = verified_document.clone();
    Ok(Some(KnowledgeUnitResponse::Document(Box::new(
        KnowledgeDocumentRevision {
            unit_id: query.unit_id,
            revision,
            lifecycle: decode(serde_json::Value::String(lifecycle))?,
            document,
            source_digests: input
                .resolved_sources
                .into_iter()
                .map(|value| value.pin.digest)
                .collect(),
            rdf_digest: verified.rdf_digest,
            unit_iri,
            revision_iri,
        },
    ))))
}

pub(crate) async fn eligible_unit(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    unit_id: Uuid,
    revision: Option<i64>,
) -> Result<KnowledgeUnitResponse> {
    let response = unit(
        tx,
        tenant,
        workspace,
        principal,
        &KnowledgeUnitQuery {
            unit_id,
            revision,
            fragment: None,
        },
    )
    .await?
    .ok_or(Error::NotFound)?;
    let KnowledgeUnitResponse::Document(document) = &response else {
        return Err(Error::NeedsContext);
    };
    if document.lifecycle != KnowledgeLifecycleState::Active {
        return Err(Error::NeedsContext);
    }
    let validation_event:Option<Uuid>=sqlx::query_scalar("SELECT id FROM knowledge_validation_events WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND unit_revision=$4 AND NOT payload_erased ORDER BY created_at DESC,id DESC LIMIT 1")
        .bind(tenant).bind(workspace).bind(unit_id).bind(document.revision).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let effective_valid_until = if let Some(event_id) = validation_event {
        let verified = event::verify_publication_event(
            tx,
            tenant,
            workspace,
            unit_id,
            document.revision,
            event_id,
            false,
        )
        .await?;
        if verified.input.planned.operation != KnowledgeLifecycleOperation::Revalidate {
            return Err(Error::InternalInvariant);
        }
        let revalidation = verified
            .input
            .planned
            .revalidation
            .as_ref()
            .ok_or(Error::InternalInvariant)?;
        let source_pin_digest = digest(&verified.input.resolved_sources)?;
        let matches:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_validation_events WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND unit_id=$4 AND unit_revision=$5 AND NOT payload_erased AND sources=$6 AND source_pin_digest=$7 AND evidence_basis=$8 AND valid_until IS NOT DISTINCT FROM $9::timestamptz AND review_due_at IS NOT DISTINCT FROM $10::timestamptz)")
            .bind(tenant).bind(workspace).bind(event_id).bind(unit_id).bind(document.revision)
            .bind(json(&revalidation.sources)?).bind(source_pin_digest).bind(&revalidation.evidence_basis)
            .bind(&revalidation.valid_until).bind(&revalidation.review_due_at)
            .fetch_one(&mut **tx).await.map_err(storage_error)?;
        if !matches {
            return Err(Error::InternalInvariant);
        }
        revalidation
            .valid_until
            .clone()
            .or_else(|| document.document.valid_until.clone())
    } else {
        document.document.valid_until.clone()
    };
    let eligible:bool=sqlx::query_scalar("SELECT ($1::timestamptz IS NULL OR $1::timestamptz<=pg_catalog.clock_timestamp()) AND ($2::timestamptz IS NULL OR $2::timestamptz>=pg_catalog.clock_timestamp())")
        .bind(&document.document.valid_from).bind(effective_valid_until.as_deref()).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if !eligible {
        return Err(Error::NeedsContext);
    }
    Ok(response)
}

pub(crate) async fn current_knowledge_output(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run_id: Uuid,
    output_id: Uuid,
    expected_digest: &str,
) -> Result<KnowledgePhaseOutput> {
    require_owner(tx, principal).await?;
    let row:Option<(i64,String,serde_json::Value)>=sqlx::query_as("SELECT o.revision,o.digest,o.output FROM knowledge_change_outputs o JOIN knowledge_change_output_bindings b ON b.tenant_id=o.tenant_id AND b.workspace_id=o.workspace_id AND b.run_id=o.run_id AND b.output_id=o.id WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.run_id=$3 AND o.id=$4 AND o.digest=$5 AND NOT o.payload_erased AND NOT b.stale").bind(tenant).bind(workspace).bind(run_id).bind(output_id).bind(expected_digest).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let (revision, digest, output) = row.ok_or(Error::InvalidSource)?;
    Ok(KnowledgePhaseOutput {
        id: output_id,
        run_id,
        revision,
        digest,
        output: decode(output)?,
        stale: false,
        stale_reason: None,
    })
}
