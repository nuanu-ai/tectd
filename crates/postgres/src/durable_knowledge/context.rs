use super::*;
use sqlx::Row;

pub(crate) async fn load_revision(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: Option<i64>,
    native: bool,
) -> Result<Option<KnowledgeUnitRevision>> {
    let row = if let Some(revision) = revision {
        sqlx::query("SELECT r.revision,(h.active AND h.accepted_revision=r.revision),r.constraint_payload,r.source_sha256,r.rdf_digest,r.publication_event_id,r.unit_iri,r.revision_iri,r.source_iri,r.publication_event_iri,e.operation,c.reason,c.authority_basis,e.actor_principal_id,e.actor_session_id,b.definition_kind,b.definition_version,b.definition_digest,h.contract_version,r.contract_version,h.payload_erased,r.payload_erased,e.payload_erased,c.payload_erased FROM knowledge_revisions r JOIN knowledge_unit_heads h ON h.tenant_id=r.tenant_id AND h.workspace_id=r.workspace_id AND h.unit_id=r.unit_id JOIN knowledge_publication_events e ON e.tenant_id=r.tenant_id AND e.workspace_id=r.workspace_id AND e.id=r.publication_event_id JOIN knowledge_changes c ON c.tenant_id=e.tenant_id AND c.workspace_id=e.workspace_id AND c.id=e.change_id JOIN knowledge_bindings b ON b.tenant_id=r.tenant_id AND b.workspace_id=r.workspace_id AND b.unit_id=r.unit_id AND b.revision=r.revision WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.unit_id=$3 AND r.revision=$4")
            .bind(tenant).bind(workspace).bind(unit).bind(revision).fetch_optional(&mut **tx).await.map_err(storage_error)?
    } else {
        sqlx::query("SELECT r.revision,h.active,r.constraint_payload,r.source_sha256,r.rdf_digest,r.publication_event_id,r.unit_iri,r.revision_iri,r.source_iri,r.publication_event_iri,e.operation,c.reason,c.authority_basis,e.actor_principal_id,e.actor_session_id,b.definition_kind,b.definition_version,b.definition_digest,h.contract_version,r.contract_version,h.payload_erased,r.payload_erased,e.payload_erased,c.payload_erased FROM knowledge_unit_heads h JOIN knowledge_revisions r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id AND r.unit_id=h.unit_id AND r.revision=h.accepted_revision JOIN knowledge_publication_events e ON e.tenant_id=r.tenant_id AND e.workspace_id=r.workspace_id AND e.id=r.publication_event_id JOIN knowledge_changes c ON c.tenant_id=e.tenant_id AND c.workspace_id=e.workspace_id AND c.id=e.change_id JOIN knowledge_bindings b ON b.tenant_id=r.tenant_id AND b.workspace_id=r.workspace_id AND b.unit_id=r.unit_id AND b.revision=r.revision WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.unit_id=$3")
            .bind(tenant).bind(workspace).bind(unit).fetch_optional(&mut **tx).await.map_err(storage_error)?
    };
    let Some(row) = row else { return Ok(None) };
    if row.get::<String, _>(18) != "dk-1" || row.get::<String, _>(19) != "dk-1" {
        return Err(Error::KnowledgeLifecycleRequired);
    }
    if row.get::<bool, _>(20)
        || row.get::<bool, _>(21)
        || row.get::<bool, _>(22)
        || row.get::<bool, _>(23)
    {
        return Err(Error::KnowledgePayloadErased);
    }
    let value = KnowledgeUnitRevision {
        unit_id: unit,
        revision: row.get(0),
        active: row.get(1),
        constraint: decode(row.get(2))?,
        source_sha256: row.get(3),
        rdf_digest: row.get(4),
        rdf_digest_method: "rdfc-1.0-sha256".into(),
        rdf_digest_scope: KnowledgeRdfDigestScope::RevisionPublicationPayload,
        publication_event_id: row.get(5),
        unit_iri: row.get(6),
        revision_iri: row.get(7),
        source_iri: row.get(8),
        publication_event_iri: row.get(9),
        publication_operation: parse_operation(row.get(10))?,
        publication_reason: row.get(11),
        publication_authority_basis: row.get(12),
        publication_actor_principal_id: row.get(13),
        publication_actor_session_id: row.get(14),
        binding_provenance: match row
            .try_get::<Option<String>, _>(15)
            .map_err(storage_error)?
        {
            Some(kind) => Some(KnowledgeBindingProvenance {
                definition_kind: serde_json::from_value(serde_json::Value::String(kind))
                    .map_err(storage_error)?,
                definition_version: row.get(16),
                definition_digest: row.get(17),
            }),
            None => None,
        },
    };
    if native {
        let rows = rdf::native_rows(
            tx,
            tenant,
            workspace,
            unit,
            value.revision,
            value.publication_event_id,
        )
        .await?;
        rdf::validate_rows(&rows, &value)?;
    }
    Ok(Some(value))
}

pub(crate) async fn context(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    query: &KnowledgeContextQuery,
    preparation: &KnowledgeMethodSnapshot,
    review: &KnowledgeMethodSnapshot,
) -> Result<KnowledgeContext> {
    let state:Option<(i64,bool,Option<String>)>=sqlx::query_as("SELECT generation,capability_ready AND tect_dk_database_identity_ready(),pgrdf_version FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2")
        .bind(tenant).bind(workspace).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let (generation, ready, version) = match state {
        Some(v) => v,
        None => {
            let (ready, version): (bool, Option<String>) =
                sqlx::query_as("SELECT capability_ready,pgrdf_version FROM tect_dk_capability()")
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(storage_error)?;
            (0, ready, version)
        }
    };
    let exact_revision = if let Some(unit) = query.unit_id {
        if !ready {
            return Err(Error::KnowledgeUnavailable);
        }
        Some(
            load_revision(tx, tenant, workspace, unit, query.revision, true)
                .await?
                .ok_or(Error::NotFound)?,
        )
    } else {
        None
    };
    Ok(KnowledgeContext {
        generation,
        capability: KnowledgeCapability {
            ready,
            profile_id: DK_PROFILE_ID.into(),
            profile_version: DK_PROFILE_VERSION.into(),
            pgrdf_version: version,
            supported_operations: vec![
                KnowledgeOperation::Create,
                KnowledgeOperation::Revise,
                KnowledgeOperation::Retract,
            ],
            supported_bindings: vec![
                KnowledgeBindingKind::Workspace,
                KnowledgeBindingKind::SlicePhase,
            ],
            lifecycle_complete: false,
        },
        preparation_method: preparation.clone(),
        review_method: review.clone(),
        exact_revision,
    })
}

pub(crate) async fn load_change(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    id: Uuid,
) -> Result<Option<KnowledgeChange>> {
    let row=sqlx::query("SELECT unit_id,change_revision,operation,stage,expected_generation,expected_unit_revision,proposed_unit_revision,proposal_digest,source_sha256,semantic_diff,baseline,proposal,binding_provenance,preparation_method,review_method,reason,authority_basis,review,publication_receipt,payload_erased FROM knowledge_changes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some(row) = row else { return Ok(None) };
    if row.get::<bool, _>(19) {
        return Err(Error::KnowledgePayloadErased);
    }
    Ok(Some(KnowledgeChange {
        id,
        unit_id: row.get(0),
        change_revision: row.get(1),
        operation: parse_operation(row.get(2))?,
        stage: stage(row.get(3))?,
        expected_generation: row.get(4),
        expected_unit_revision: row.get(5),
        proposed_unit_revision: row.get(6),
        proposal_digest: row.get(7),
        source_sha256: row.get(8),
        semantic_diff: row.get(9),
        baseline: row
            .try_get::<Option<serde_json::Value>, _>(10)
            .map_err(storage_error)?
            .map(decode)
            .transpose()?,
        proposal: row
            .try_get::<Option<serde_json::Value>, _>(11)
            .map_err(storage_error)?
            .map(decode)
            .transpose()?,
        binding_provenance: row
            .try_get::<Option<serde_json::Value>, _>(12)
            .map_err(storage_error)?
            .map(decode)
            .transpose()?,
        preparation_method: decode(row.get(13))?,
        review_method: decode(row.get(14))?,
        reason: row.get(15),
        authority_basis: row.get(16),
        review: row
            .try_get::<Option<serde_json::Value>, _>(17)
            .map_err(storage_error)?
            .map(decode)
            .transpose()?,
        publication_receipt: row
            .try_get::<Option<serde_json::Value>, _>(18)
            .map_err(storage_error)?
            .map(decode)
            .transpose()?,
    }))
}
