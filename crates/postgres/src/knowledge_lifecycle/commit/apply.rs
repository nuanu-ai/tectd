use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[allow(clippy::too_many_arguments)]
pub(super) async fn apply_operation(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    principal: Uuid,
    session: Uuid,
    operation: &KnowledgePlannedOperation,
    created: &BTreeMap<Uuid, Uuid>,
) -> Result<KnowledgeAppliedOperationReceipt> {
    let head = lock_head(tx, tenant, workspace, operation).await?;
    let revision = match operation.operation {
        KnowledgeLifecycleOperation::Create => 1,
        KnowledgeLifecycleOperation::Revise => head.as_ref().ok_or(Error::NotFound)?.0 + 1,
        _ => head.as_ref().ok_or(Error::NotFound)?.0,
    };
    let event = Uuid::new_v4();
    let successor = successor(tx, tenant, workspace, principal, operation, created).await?;
    if operation.operation == KnowledgeLifecycleOperation::Erase {
        let sequence: i64 = sqlx::query_scalar("UPDATE durable_knowledge_capability SET erasure_sequence=erasure_sequence+1 RETURNING erasure_sequence")
            .fetch_one(&mut **tx).await.map_err(storage_error)?;
        sqlx::query("UPDATE knowledge_unit_heads SET active=false,lifecycle='erasure_pending',last_event_id=$4,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
            .bind(tenant).bind(workspace).bind(operation.unit_id).bind(event).execute(&mut **tx).await.map_err(storage_error)?;
        sqlx::query("UPDATE knowledge_bindings SET active=false WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
            .bind(tenant).bind(workspace).bind(operation.unit_id).execute(&mut **tx).await.map_err(storage_error)?;
        sqlx::query("INSERT INTO knowledge_suppression_ledger(tenant_id,workspace_id,unit_id,change_id,run_id,request_id,event_id,erasure_sequence,lifecycle,owned_live_copies_status,restore_safe_status) SELECT $1,$2,$3,$4,r.id,c.request_id,$5,$6,'erasure_pending','pending','pending' FROM knowledge_change_runs r JOIN knowledge_lifecycle_changes c ON c.tenant_id=r.tenant_id AND c.workspace_id=r.workspace_id AND c.id=r.change_id WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.change_id=$4")
            .bind(tenant).bind(workspace).bind(operation.unit_id).bind(change).bind(event).bind(sequence).execute(&mut **tx).await.map_err(storage_error)?;
        let unit_iri = format!(
            "urn:tect:dk:unit:{tenant}:{workspace}:{}",
            operation.unit_id
        );
        return Ok(KnowledgeAppliedOperationReceipt {
            operation_id: operation.operation_id,
            unit_id: operation.unit_id,
            operation: operation.operation,
            revision: None,
            event_id: event,
            unit_iri,
            revision_iri: None,
            event_iri: format!("urn:tect:dk:event:{tenant}:{workspace}:{event}"),
            rdf_digest: String::new(),
            rdf_digest_method: "none-erasure".into(),
            rdf_digest_scope: KnowledgeRdfDigestScope::LifecycleEventPayload,
        });
    }
    let sources = resolved_sources(tx, tenant, workspace, change, operation).await?;
    if operation.operation == KnowledgeLifecycleOperation::Revalidate {
        require_new_revalidation_evidence(
            tx,
            tenant,
            workspace,
            operation.unit_id,
            revision,
            &sources,
        )
        .await?;
    }
    let input = rdf::RdfPublicationInput {
        tenant,
        workspace,
        change_id: change,
        event_id: event,
        content_revision: revision,
        planned: operation.clone(),
        principal_id: principal,
        session_id: session,
        resolved_sources: sources.clone(),
        successor_unit: successor,
        include_empty_planning_briefs: true,
    };
    let document = rdf::build(&input)?;
    let rdf_digest =
        rdf::native_publish(tx, tenant, workspace, event, operation.operation, &document).await?;
    let scope = if matches!(
        operation.operation,
        KnowledgeLifecycleOperation::Create | KnowledgeLifecycleOperation::Revise
    ) {
        KnowledgeRdfDigestScope::RevisionPublicationPayload
    } else {
        KnowledgeRdfDigestScope::LifecycleEventPayload
    };
    sqlx::query("INSERT INTO knowledge_publication_events(id,tenant_id,workspace_id,unit_id,unit_revision,change_id,lifecycle_change_id,operation_id,operation,actor_principal_id,actor_session_id,rdf_digest,rdf_digest_method,rdf_digest_scope,unit_iri,revision_iri,event_iri,contract_version,event_payload) VALUES($1,$2,$3,$4,$5,NULL,$6,$7,$8,$9,$10,$11,'rdfc-1.0-sha256',$12,$13,$14,$15,'dk-2',$16)")
        .bind(event).bind(tenant).bind(workspace).bind(operation.unit_id).bind(revision).bind(change).bind(operation.operation_id).bind(enum_text(&operation.operation)?).bind(principal).bind(session).bind(&rdf_digest).bind(enum_text(&scope)?).bind(&document.refs.unit).bind(&document.refs.revision).bind(&document.refs.event).bind(json(&input)?).execute(&mut **tx).await.map_err(storage_error)?;
    match operation.operation {
        KnowledgeLifecycleOperation::Create | KnowledgeLifecycleOperation::Revise => {
            let document_payload = operation.document.as_ref().ok_or(Error::InvalidArguments)?;
            let fingerprint = digest(document_payload)?;
            let existing:Option<Uuid>=sqlx::query_scalar("SELECT unit_id FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 AND lifecycle='active' AND proposal_fingerprint=$3 AND unit_id<>$4 LIMIT 1 FOR UPDATE").bind(tenant).bind(workspace).bind(&fingerprint).bind(operation.unit_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
            if existing.is_some() {
                return Err(Error::InputConflict);
            }
            if operation.operation == KnowledgeLifecycleOperation::Revise {
                sqlx::query("UPDATE knowledge_bindings SET active=false WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND active").bind(tenant).bind(workspace).bind(operation.unit_id).execute(&mut **tx).await.map_err(storage_error)?;
            }
            sqlx::query("INSERT INTO knowledge_revisions(tenant_id,workspace_id,unit_id,revision,constraint_payload,source_sha256,rdf_digest,rdf_digest_method,rdf_digest_scope,publication_event_id,unit_iri,revision_iri,source_iri,publication_event_iri,contract_version,document_payload,access_scope) VALUES($1,$2,$3,$4,NULL,$5,$6,'rdfc-1.0-sha256','revision_publication_payload',$7,$8,$9,$10,$11,'dk-2',$12,$13)")
                .bind(tenant).bind(workspace).bind(operation.unit_id).bind(revision).bind(digest(&sources)?).bind(&rdf_digest).bind(event).bind(&document.refs.unit).bind(&document.refs.revision).bind(format!("{}:source",document.refs.revision)).bind(&document.refs.event).bind(json(document_payload)?).bind(enum_text(&document_payload.access_scope)?).execute(&mut **tx).await.map_err(storage_error)?;
            insert_bindings(
                tx,
                tenant,
                workspace,
                operation.unit_id,
                revision,
                &document_payload.bindings,
                &operation.binding_pins,
            )
            .await?;
            sqlx::query("INSERT INTO knowledge_unit_heads(tenant_id,workspace_id,unit_id,accepted_revision,active,proposal_fingerprint,last_event_id,contract_version,lifecycle,access_scope) VALUES($1,$2,$3,$4,true,$5,$6,'dk-2','active',$7) ON CONFLICT(tenant_id,workspace_id,unit_id) DO UPDATE SET accepted_revision=EXCLUDED.accepted_revision,active=true,proposal_fingerprint=EXCLUDED.proposal_fingerprint,last_event_id=EXCLUDED.last_event_id,contract_version='dk-2',lifecycle='active',access_scope=EXCLUDED.access_scope,updated_at=pg_catalog.clock_timestamp()")
                .bind(tenant).bind(workspace).bind(operation.unit_id).bind(revision).bind(fingerprint).bind(event).bind(enum_text(&document_payload.access_scope)?).execute(&mut **tx).await.map_err(storage_error)?;
        }
        KnowledgeLifecycleOperation::Revalidate => {
            let value = operation
                .revalidation
                .as_ref()
                .ok_or(Error::InvalidArguments)?;
            sqlx::query("INSERT INTO knowledge_validation_events(id,tenant_id,workspace_id,unit_id,unit_revision,lifecycle_change_id,operation_id,sources,source_pin_digest,evidence_basis,valid_until,review_due_at,actor_principal_id,actor_session_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11::timestamptz,$12::timestamptz,$13,$14)")
                .bind(event).bind(tenant).bind(workspace).bind(operation.unit_id).bind(revision).bind(change).bind(operation.operation_id).bind(json(&value.sources)?).bind(digest(&sources)?).bind(&value.evidence_basis).bind(&value.valid_until).bind(&value.review_due_at).bind(principal).bind(session).execute(&mut **tx).await.map_err(storage_error)?;
            sqlx::query("UPDATE knowledge_unit_heads SET last_validation_event_id=$4,last_event_id=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3").bind(tenant).bind(workspace).bind(operation.unit_id).bind(event).execute(&mut **tx).await.map_err(storage_error)?;
        }
        KnowledgeLifecycleOperation::Supersede => {
            apply_supersession(
                tx,
                tenant,
                workspace,
                operation,
                successor.ok_or(Error::InvalidArguments)?,
                event,
            )
            .await?
        }
        KnowledgeLifecycleOperation::Retract => {
            sqlx::query("UPDATE knowledge_unit_heads SET active=false,lifecycle='retracted',last_event_id=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3").bind(tenant).bind(workspace).bind(operation.unit_id).bind(event).execute(&mut **tx).await.map_err(storage_error)?;
            sqlx::query("UPDATE knowledge_bindings SET active=false WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3").bind(tenant).bind(workspace).bind(operation.unit_id).execute(&mut **tx).await.map_err(storage_error)?;
        }
        KnowledgeLifecycleOperation::Erase => unreachable!(),
    }
    sqlx::query("UPDATE knowledge_change_operations SET applied_event_id=$4,applied_revision=$5 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(operation.operation_id).bind(event).bind(revision).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(KnowledgeAppliedOperationReceipt {
        operation_id: operation.operation_id,
        unit_id: operation.unit_id,
        operation: operation.operation,
        revision: Some(revision),
        event_id: event,
        unit_iri: document.refs.unit,
        revision_iri: matches!(
            operation.operation,
            KnowledgeLifecycleOperation::Create | KnowledgeLifecycleOperation::Revise
        )
        .then_some(document.refs.revision),
        event_iri: document.refs.event,
        rdf_digest,
        rdf_digest_method: "rdfc-1.0-sha256".into(),
        rdf_digest_scope: scope,
    })
}

async fn apply_supersession(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    operation: &KnowledgePlannedOperation,
    successor: Uuid,
    event: Uuid,
) -> Result<()> {
    let current:i64=sqlx::query_scalar("SELECT count(*) FROM knowledge_bindings WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND active").bind(tenant).bind(workspace).bind(operation.unit_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let mut ids = Vec::new();
    let mut unique = BTreeSet::new();
    for binding in &operation.replacement_bindings {
        if !matches!(
            binding.version_resolution,
            KnowledgeBindingVersion::CurrentAccepted
        ) {
            return Err(Error::InvalidArguments);
        }
        let value = json(binding)?;
        if !unique.insert(digest(&value)?) {
            return Err(Error::InvalidArguments);
        }
        let id:Option<Uuid>=sqlx::query_scalar("SELECT id FROM knowledge_bindings WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND active AND jsonb_build_object('target',CASE binding_kind WHEN 'workspace' THEN jsonb_build_object('kind','workspace') WHEN 'program' THEN jsonb_build_object('kind','program','program_id',program_id) WHEN 'scope' THEN jsonb_build_object('kind','scope','scope_id',scope_id) WHEN 'slice' THEN jsonb_build_object('kind','slice','scope_id',scope_id,'slice_id',slice_id) ELSE jsonb_build_object('kind','slice_phase','scope_id',scope_id,'slice_id',slice_id,'phase_id',phase_id) END,'purpose',purpose,'version_resolution',CASE version_resolution WHEN 'current_accepted' THEN jsonb_build_object('kind','current_accepted') ELSE jsonb_build_object('kind','pinned_revision','revision',pinned_revision) END)=$4 LIMIT 1").bind(tenant).bind(workspace).bind(operation.unit_id).bind(value).fetch_optional(&mut **tx).await.map_err(storage_error)?;
        ids.push(id.ok_or(Error::InputConflict)?);
    }
    sqlx::query("UPDATE knowledge_bindings SET active=false WHERE tenant_id=$1 AND workspace_id=$2 AND id=ANY($3)").bind(tenant).bind(workspace).bind(&ids).execute(&mut **tx).await.map_err(storage_error)?;
    let successor_revision:i64=sqlx::query_scalar("SELECT accepted_revision FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3").bind(tenant).bind(workspace).bind(successor).fetch_one(&mut **tx).await.map_err(storage_error)?;
    insert_bindings(
        tx,
        tenant,
        workspace,
        successor,
        successor_revision,
        &operation.replacement_bindings,
        &operation.binding_pins,
    )
    .await?;
    let full = current == ids.len() as i64;
    sqlx::query("INSERT INTO knowledge_supersessions(id,tenant_id,workspace_id,predecessor_unit_id,successor_unit_id,event_id,replacement_binding_ids,full_replacement) VALUES($1,$2,$3,$4,$5,$6,$7,$8)").bind(Uuid::new_v4()).bind(tenant).bind(workspace).bind(operation.unit_id).bind(successor).bind(event).bind(&ids).bind(full).execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE knowledge_unit_heads SET lifecycle=$4,active=$5,last_event_id=$6 WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3").bind(tenant).bind(workspace).bind(operation.unit_id).bind(if full {"superseded"} else {"active"}).bind(!full).bind(event).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(())
}
