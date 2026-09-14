use super::*;

type RevisionGuardRow = (i64, String, String, Option<String>, String, String, Uuid);
type PublicationEventRow = (
    Option<serde_json::Value>,
    Option<String>,
    Uuid,
    i64,
    String,
    Uuid,
    Uuid,
    bool,
);

pub(crate) struct VerifiedPublicationEvent {
    pub input: rdf::RdfPublicationInput,
    pub rdf_digest: String,
}

pub(crate) async fn verify_revision_guard(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    guard: &KnowledgeRevisionGuard,
) -> Result<()> {
    let row: Option<RevisionGuardRow> =
        sqlx::query_as(
            "SELECT h.accepted_revision,h.lifecycle,r.contract_version,r.rdf_digest,r.unit_iri,r.revision_iri,r.publication_event_id \
             FROM knowledge_unit_heads h JOIN knowledge_revisions r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id AND r.unit_id=h.unit_id AND r.revision=h.accepted_revision \
             WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.unit_id=$3 AND NOT h.payload_erased AND NOT r.payload_erased",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(guard.unit_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(storage_error)?;
    let Some((revision, lifecycle, contract, rdf_digest, unit_iri, revision_iri, event)) = row
    else {
        return Err(Error::ContextChanged);
    };
    if revision != guard.revision
        || lifecycle != enum_text(&guard.lifecycle)?
        || rdf_digest.as_deref() != Some(guard.rdf_digest.as_str())
        || unit_iri != guard.unit_iri
        || revision_iri != guard.revision_iri
    {
        return Err(Error::ContextChanged);
    }
    if contract == "dk-2" {
        let verified =
            verify_publication_event(tx, tenant, workspace, guard.unit_id, revision, event, true)
                .await?;
        if verified.rdf_digest != guard.rdf_digest {
            return Err(Error::ContextChanged);
        }
    } else if contract == "dk-1" {
        let revision = crate::durable_knowledge::context::load_revision(
            tx,
            tenant,
            workspace,
            guard.unit_id,
            Some(guard.revision),
            true,
        )
        .await?
        .ok_or(Error::ContextChanged)?;
        if revision.rdf_digest != guard.rdf_digest
            || revision.unit_iri != guard.unit_iri
            || revision.revision_iri != guard.revision_iri
        {
            return Err(Error::ContextChanged);
        }
    } else {
        return Err(Error::InternalInvariant);
    }
    Ok(())
}

pub(crate) async fn verify_publication_event(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: i64,
    event: Uuid,
    include_revision: bool,
) -> Result<VerifiedPublicationEvent> {
    let verified = verify_native_publication_event(
        tx,
        tenant,
        workspace,
        unit,
        revision,
        event,
        include_revision,
    )
    .await?;
    verify_receipt_proof(tx, tenant, workspace, &verified).await?;
    Ok(verified)
}

pub(crate) async fn verify_native_publication_event(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: i64,
    event: Uuid,
    include_revision: bool,
) -> Result<VerifiedPublicationEvent> {
    let row: Option<PublicationEventRow> = sqlx::query_as(
        "SELECT event_payload,rdf_digest,unit_id,unit_revision,operation,lifecycle_change_id,operation_id,payload_erased \
         FROM knowledge_publication_events WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND contract_version='dk-2'",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(event)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let (
        payload,
        event_digest,
        event_unit,
        event_revision,
        operation,
        change,
        operation_id,
        erased,
    ) = row.ok_or(Error::InternalInvariant)?;
    if erased {
        return Err(Error::KnowledgePayloadErased);
    }
    let input: rdf::RdfPublicationInput = decode(payload.ok_or(Error::InternalInvariant)?)?;
    let event_digest = event_digest.ok_or(Error::InternalInvariant)?;
    if event_unit != unit
        || event_revision != revision
        || input.tenant != tenant
        || input.workspace != workspace
        || input.change_id != change
        || input.event_id != event
        || input.content_revision != revision
        || input.planned.unit_id != unit
        || input.planned.operation_id != operation_id
        || enum_text(&input.planned.operation)? != operation
    {
        return Err(Error::InternalInvariant);
    }
    let expected = rdf::build(&input)?;
    let rows = rdf::native_rows(
        tx,
        tenant,
        workspace,
        unit,
        revision,
        event,
        include_revision,
    )
    .await?;
    rdf::validate_rows(&rows, &expected)?;

    Ok(VerifiedPublicationEvent {
        input,
        rdf_digest: event_digest,
    })
}

async fn verify_receipt_proof(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    verified: &VerifiedPublicationEvent,
) -> Result<()> {
    let stored: Option<(Option<serde_json::Value>, Option<serde_json::Value>)> = sqlx::query_as(
        "SELECT publisher_receipt,erased_publisher_receipt FROM knowledge_change_runs \
             WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(verified.input.change_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let receipt = match stored.ok_or(Error::InternalInvariant)? {
        (Some(value), None) => {
            let mut full: KnowledgePublisherReceipt = decode(value)?;
            let stored_digest = std::mem::take(&mut full.digest);
            if digest(&full)? != stored_digest {
                return Err(Error::InternalInvariant);
            }
            full.applied_operations
                .into_iter()
                .find(|value| value.operation_id == verified.input.planned.operation_id)
        }
        (None, Some(value)) => {
            let erased: KnowledgeErasedPublisherReceipt = decode(value)?;
            erased.operations.into_iter().find_map(|value| match value {
                KnowledgeRetainedOperationReceipt::Intact(receipt)
                    if receipt.operation_id == verified.input.planned.operation_id =>
                {
                    Some(receipt)
                }
                _ => None,
            })
        }
        _ => return Err(Error::InternalInvariant),
    }
    .ok_or(Error::InternalInvariant)?;
    let expected_revision = Some(verified.input.content_revision);
    let expected_scope = if matches!(
        verified.input.planned.operation,
        KnowledgeLifecycleOperation::Create | KnowledgeLifecycleOperation::Revise
    ) {
        KnowledgeRdfDigestScope::RevisionPublicationPayload
    } else {
        KnowledgeRdfDigestScope::LifecycleEventPayload
    };
    let expected_unit_iri = format!(
        "urn:tect:dk:unit:{tenant}:{workspace}:{}",
        verified.input.planned.unit_id
    );
    let expected_event_iri = format!(
        "urn:tect:dk:event:{tenant}:{workspace}:{}",
        verified.input.event_id
    );
    let expected_revision_iri = matches!(
        verified.input.planned.operation,
        KnowledgeLifecycleOperation::Create | KnowledgeLifecycleOperation::Revise
    )
    .then(|| {
        format!(
            "{expected_unit_iri}:revision:{}",
            verified.input.content_revision
        )
    });
    if receipt.unit_id != verified.input.planned.unit_id
        || receipt.operation != verified.input.planned.operation
        || receipt.revision != expected_revision
        || receipt.event_id != verified.input.event_id
        || receipt.unit_iri != expected_unit_iri
        || receipt.revision_iri != expected_revision_iri
        || receipt.event_iri != expected_event_iri
        || receipt.rdf_digest != verified.rdf_digest
        || receipt.rdf_digest_method != "rdfc-1.0-sha256"
        || receipt.rdf_digest_scope != expected_scope
    {
        return Err(Error::InternalInvariant);
    }
    Ok(())
}
