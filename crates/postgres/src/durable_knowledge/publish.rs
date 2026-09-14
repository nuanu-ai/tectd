use super::*;

pub(crate) async fn publish(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Uuid,
    request: &PublishKnowledgeChange,
) -> Result<PublishKnowledgeChangeOutcome> {
    let payload = json(request)?;
    if let Some(prior) = receipt::<PublishKnowledgeChangeOutcome>(
        tx,
        tenant,
        workspace,
        "publish",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(match prior {
            PublishKnowledgeChangeOutcome::Published(v)
            | PublishKnowledgeChangeOutcome::Replay(v) => PublishKnowledgeChangeOutcome::Replay(v),
            other => other,
        });
    }
    // Global native DDL gate precedes workspace/head locks and every native RDF read.
    publisher_gate(tx).await?;
    let (generation, ready, _) = lock_state(tx, tenant, workspace).await?;
    if let Some(prior) = receipt::<PublishKnowledgeChangeOutcome>(
        tx,
        tenant,
        workspace,
        "publish",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(match prior {
            PublishKnowledgeChangeOutcome::Published(v)
            | PublishKnowledgeChangeOutcome::Replay(v) => PublishKnowledgeChangeOutcome::Replay(v),
            other => other,
        });
    }
    if !ready {
        return Err(Error::KnowledgeUnavailable);
    }
    let change = context::load_change(tx, tenant, workspace, request.change_id)
        .await?
        .ok_or(Error::NotFound)?;
    if change.change_revision != request.change_revision
        || change.proposal_digest != request.proposal_digest
        || change.stage != KnowledgeChangeStage::ReadyToPublish
    {
        return Err(Error::StaleRevision);
    }
    if generation != change.expected_generation {
        return Err(Error::StaleContext);
    }
    let head:Option<(i64,bool,String)>=sqlx::query_as("SELECT accepted_revision,active,proposal_fingerprint FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(change.unit_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    match change.operation {
        KnowledgeOperation::Create if head.is_some() => return Err(Error::InputConflict),
        KnowledgeOperation::Create => {}
        KnowledgeOperation::Revise
            if head.as_ref().map(|v| v.0) != change.expected_unit_revision =>
        {
            return Err(Error::StaleRevision);
        }
        KnowledgeOperation::Retract
            if head.as_ref().map(|v| (v.0, v.1))
                != change.expected_unit_revision.map(|v| (v, true)) =>
        {
            return Err(Error::StaleRevision);
        }
        _ => {}
    }
    let draft = change
        .proposal
        .as_ref()
        .or_else(|| change.baseline.as_ref().map(|v| &v.constraint))
        .ok_or(Error::InternalInvariant)?;
    let fingerprint = fingerprint(draft)?;
    if change.operation!=KnowledgeOperation::Retract
        && let Some(existing)=sqlx::query_scalar::<_,Uuid>("SELECT unit_id FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 AND active AND proposal_fingerprint=$3 AND unit_id<>$4 LIMIT 1 FOR UPDATE")
            .bind(tenant).bind(workspace).bind(&fingerprint).bind(change.unit_id).fetch_optional(&mut **tx).await.map_err(storage_error)? {
        let outcome=PublishKnowledgeChangeOutcome::Duplicate{existing_unit_id:existing}; save_receipt(tx,tenant,workspace,"publish",request.request_id,session,&payload,&outcome).await?; return Ok(outcome)
    }
    let event = Uuid::new_v4();
    let source_sha = sha256(draft.source.text.as_bytes());
    let rdf_document = if change.operation == KnowledgeOperation::Retract {
        rdf::event_document(
            tenant,
            workspace,
            change.unit_id,
            change.proposed_unit_revision,
            event,
            change.operation,
            &change.reason,
            &change.authority_basis,
            principal,
            session,
        )?
    } else {
        rdf::revision_document(
            tenant,
            workspace,
            change.unit_id,
            change.proposed_unit_revision,
            event,
            change.operation,
            draft,
            change.binding_provenance.as_ref(),
            &source_sha,
            &change.reason,
            &change.authority_basis,
            principal,
            session,
        )?
    };
    let rdf_digest = rdf::native_publish(
        tx,
        tenant,
        workspace,
        event,
        change.operation,
        &rdf_document.payload,
        &rdf_document.stable_payload,
    )
    .await?;
    let refs = rdf_document.refs;
    let digest_scope = if change.operation == KnowledgeOperation::Retract {
        "lifecycle_event_payload"
    } else {
        "revision_publication_payload"
    };
    sqlx::query("INSERT INTO knowledge_publication_events(id,tenant_id,workspace_id,unit_id,unit_revision,change_id,operation,actor_principal_id,actor_session_id,rdf_digest,rdf_digest_method,rdf_digest_scope,unit_iri,revision_iri,event_iri) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'rdfc-1.0-sha256',$11,$12,$13,$14)")
        .bind(event).bind(tenant).bind(workspace).bind(change.unit_id).bind(change.proposed_unit_revision).bind(change.id).bind(operation(change.operation)).bind(principal).bind(session).bind(&rdf_digest).bind(digest_scope).bind(&refs.unit).bind(&refs.revision).bind(&refs.event).execute(&mut **tx).await.map_err(storage_error)?;
    match change.operation {
        KnowledgeOperation::Create | KnowledgeOperation::Revise => {
            if change.operation == KnowledgeOperation::Revise {
                sqlx::query("UPDATE knowledge_bindings SET active=false WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND active").bind(tenant).bind(workspace).bind(change.unit_id).execute(&mut **tx).await.map_err(storage_error)?;
            }
            sqlx::query("INSERT INTO knowledge_revisions(tenant_id,workspace_id,unit_id,revision,constraint_payload,source_sha256,rdf_digest,rdf_digest_method,rdf_digest_scope,publication_event_id,unit_iri,revision_iri,source_iri,publication_event_iri) VALUES($1,$2,$3,$4,$5,$6,$7,'rdfc-1.0-sha256','revision_publication_payload',$8,$9,$10,$11,$12)")
                .bind(tenant).bind(workspace).bind(change.unit_id).bind(change.proposed_unit_revision).bind(json(draft)?).bind(&source_sha).bind(&rdf_digest).bind(event).bind(&refs.unit).bind(&refs.revision).bind(&refs.source).bind(&refs.event).execute(&mut **tx).await.map_err(storage_error)?;
            let (kind, scope, slice, phase) = match &draft.binding {
                KnowledgeBinding::Workspace => ("workspace", None, None, None),
                KnowledgeBinding::SlicePhase {
                    scope_id,
                    slice_id,
                    phase_id,
                } => (
                    "slice_phase",
                    Some(*scope_id),
                    Some(*slice_id),
                    Some(phase_id.clone()),
                ),
            };
            let provenance = change.binding_provenance.as_ref();
            sqlx::query("INSERT INTO knowledge_bindings(tenant_id,workspace_id,unit_id,revision,binding_kind,scope_id,slice_id,phase_id,definition_kind,definition_version,definition_digest,active) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,true)")
                .bind(tenant).bind(workspace).bind(change.unit_id).bind(change.proposed_unit_revision).bind(kind).bind(scope).bind(slice).bind(phase).bind(provenance.map(|v|v.definition_kind.as_str())).bind(provenance.map(|v|v.definition_version.as_str())).bind(provenance.map(|v|v.definition_digest.as_str())).execute(&mut **tx).await.map_err(storage_error)?;
            sqlx::query("INSERT INTO knowledge_unit_heads(tenant_id,workspace_id,unit_id,accepted_revision,active,proposal_fingerprint,last_event_id) VALUES($1,$2,$3,$4,true,$5,$6) ON CONFLICT(tenant_id,workspace_id,unit_id) DO UPDATE SET accepted_revision=EXCLUDED.accepted_revision,active=true,contract_version='dk-1',lifecycle='active',access_scope='workspace_members',proposal_fingerprint=EXCLUDED.proposal_fingerprint,last_event_id=EXCLUDED.last_event_id,updated_at=pg_catalog.clock_timestamp()")
                .bind(tenant).bind(workspace).bind(change.unit_id).bind(change.proposed_unit_revision).bind(&fingerprint).bind(event).execute(&mut **tx).await.map_err(storage_error)?;
        }
        KnowledgeOperation::Retract => {
            sqlx::query("UPDATE knowledge_unit_heads SET active=false,lifecycle='retracted',last_event_id=$4,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
                .bind(tenant).bind(workspace).bind(change.unit_id).bind(event).execute(&mut **tx).await.map_err(storage_error)?;
            sqlx::query("UPDATE knowledge_bindings SET active=false WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND active")
                .bind(tenant).bind(workspace).bind(change.unit_id).execute(&mut **tx).await.map_err(storage_error)?;
        }
    }
    let next_generation = generation.checked_add(1).ok_or(Error::StorageUnavailable)?;
    sqlx::query(
        "UPDATE workspace_knowledge_state SET generation=$3 WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(next_generation)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    crate::knowledge_search::project_legacy(
        tx,
        tenant,
        workspace,
        principal,
        change.unit_id,
        next_generation,
    )
    .await?;
    sqlx::query("INSERT INTO knowledge_effect_outbox(id,tenant_id,workspace_id,publication_event_id,effect) VALUES($1,$2,$3,$4,'invalidate_phase_context')").bind(Uuid::new_v4()).bind(tenant).bind(workspace).bind(event).execute(&mut **tx).await.map_err(storage_error)?;
    let receipt_value = KnowledgePublicationReceipt {
        id: Uuid::new_v4(),
        change_id: change.id,
        unit_id: change.unit_id,
        operation: change.operation,
        unit_revision: change.proposed_unit_revision,
        event_id: event,
        workspace_generation: next_generation,
        rdf_digest,
        rdf_digest_method: "rdfc-1.0-sha256".into(),
        rdf_digest_scope: if change.operation == KnowledgeOperation::Retract {
            KnowledgeRdfDigestScope::LifecycleEventPayload
        } else {
            KnowledgeRdfDigestScope::RevisionPublicationPayload
        },
        unit_iri: refs.unit,
        revision_iri: refs.revision,
        event_iri: refs.event,
        delivery_eligible: change.operation != KnowledgeOperation::Retract,
        effects_status: "recorded".into(),
    };
    sqlx::query("UPDATE knowledge_changes SET stage='committed',publication_receipt=$4,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(change.id).bind(json(&receipt_value)?).execute(&mut **tx).await.map_err(storage_error)?;
    let outcome = PublishKnowledgeChangeOutcome::Published(receipt_value);
    save_receipt(
        tx,
        tenant,
        workspace,
        "publish",
        request.request_id,
        session,
        &payload,
        &outcome,
    )
    .await?;
    Ok(outcome)
}
