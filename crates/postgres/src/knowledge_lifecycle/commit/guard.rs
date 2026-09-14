use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn replay_outcome(value: CommitKnowledgeChangeOutcome) -> CommitKnowledgeChangeOutcome {
    match value {
        CommitKnowledgeChangeOutcome::Applied(receipt)
        | CommitKnowledgeChangeOutcome::Replay(receipt) => {
            CommitKnowledgeChangeOutcome::Replay(receipt)
        }
        CommitKnowledgeChangeOutcome::AppliedErased(_) => {
            unreachable!("erased command payloads are never replayed")
        }
    }
}

pub(super) fn topo(
    mut values: Vec<KnowledgePlannedOperation>,
) -> Result<Vec<KnowledgePlannedOperation>> {
    let ids = values
        .iter()
        .map(|value| value.operation_id)
        .collect::<BTreeSet<_>>();
    if values.iter().any(|value| {
        value
            .dependency_operation_ids
            .iter()
            .any(|id| !ids.contains(id))
    }) {
        return Err(Error::InvalidArguments);
    }
    let mut complete = BTreeSet::new();
    let mut ordered = Vec::with_capacity(values.len());
    while !values.is_empty() {
        let Some(index) = values.iter().position(|value| {
            value
                .dependency_operation_ids
                .iter()
                .all(|id| complete.contains(id))
        }) else {
            return Err(Error::InvalidArguments);
        };
        let value = values.remove(index);
        complete.insert(value.operation_id);
        ordered.push(value);
    }
    Ok(ordered)
}

pub(super) async fn lock_head(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    operation: &KnowledgePlannedOperation,
) -> Result<Option<(i64, String, String)>> {
    let suppressed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_suppression_ledger WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3)")
        .bind(tenant).bind(workspace).bind(operation.unit_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if suppressed {
        return Err(Error::KnowledgePayloadErased);
    }
    let head: Option<(i64,String,String)> = sqlx::query_as("SELECT accepted_revision,lifecycle,contract_version FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(operation.unit_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    match operation.operation {
        KnowledgeLifecycleOperation::Create if head.is_none() => Ok(None),
        KnowledgeLifecycleOperation::Create => Err(Error::InputConflict),
        _ => {
            let value = head.ok_or(Error::NotFound)?;
            if operation.expected_revision != Some(value.0)
                || operation
                    .expected_lifecycle
                    .as_ref()
                    .map(enum_text)
                    .transpose()?
                    .as_deref()
                    != Some(value.1.as_str())
            {
                return Err(Error::StaleRevision);
            }
            let lifecycle_allowed = match operation.operation {
                KnowledgeLifecycleOperation::Revise => {
                    matches!(value.1.as_str(), "active" | "retracted")
                }
                KnowledgeLifecycleOperation::Revalidate
                | KnowledgeLifecycleOperation::Supersede
                | KnowledgeLifecycleOperation::Retract => value.1 == "active",
                KnowledgeLifecycleOperation::Erase => {
                    matches!(value.1.as_str(), "active" | "retracted" | "superseded")
                }
                KnowledgeLifecycleOperation::Create => unreachable!(),
            };
            if !lifecycle_allowed {
                return Err(Error::NeedsContext);
            }
            Ok(Some(value))
        }
    }
}

pub(super) async fn resolved_sources(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    operation: &KnowledgePlannedOperation,
) -> Result<Vec<rdf::ResolvedSourcePayload>> {
    let sources = match operation.operation {
        KnowledgeLifecycleOperation::Create | KnowledgeLifecycleOperation::Revise => {
            &operation
                .document
                .as_ref()
                .ok_or(Error::InvalidArguments)?
                .sources
        }
        KnowledgeLifecycleOperation::Revalidate => {
            &operation
                .revalidation
                .as_ref()
                .ok_or(Error::InvalidArguments)?
                .sources
        }
        _ => return Ok(Vec::new()),
    };
    let all_sources: Vec<KnowledgeSourceRef> = decode(
        sqlx::query_scalar(
            "SELECT sources FROM knowledge_lifecycle_changes \
             WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND NOT payload_erased",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(change)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?,
    )?;
    let all_resolved =
        phase_data::resolve_sources(tx, tenant, workspace, change, &all_sources).await?;
    let mut operation_sources = Vec::with_capacity(sources.len());
    for (local_index, source) in sources.iter().enumerate() {
        let matches = all_sources
            .iter()
            .enumerate()
            .filter_map(|(index, candidate)| (candidate == source).then_some(index))
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(Error::InvalidSource);
        }
        let mut resolved = all_resolved[matches[0]].clone();
        resolved.pin.source_index = local_index as u32;
        operation_sources.push(resolved);
    }
    Ok(operation_sources)
}

pub(super) async fn require_new_revalidation_evidence(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: i64,
    sources: &[rdf::ResolvedSourcePayload],
) -> Result<()> {
    let payloads: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT event_payload FROM knowledge_publication_events \
         WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND unit_revision=$4 \
         AND contract_version='dk-2' AND operation IN ('create','revise','revalidate') \
         AND NOT payload_erased ORDER BY created_at,id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(unit)
    .bind(revision)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    let mut accepted = BTreeSet::new();
    for payload in payloads {
        let input: rdf::RdfPublicationInput = decode(payload)?;
        accepted.extend(
            input
                .resolved_sources
                .into_iter()
                .map(|source| source.pin.digest),
        );
    }
    if sources
        .iter()
        .all(|source| accepted.contains(&source.pin.digest))
    {
        return Err(Error::NeedsContext);
    }
    Ok(())
}

pub(super) async fn successor(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    operation: &KnowledgePlannedOperation,
    created: &BTreeMap<Uuid, Uuid>,
) -> Result<Option<Uuid>> {
    let Some(value) = &operation.successor else {
        return Ok(None);
    };
    let (unit, same_change) = match (value.unit_id, value.operation_id) {
        (Some(unit), None) => (unit, false),
        (None, Some(id)) => (*created.get(&id).ok_or(Error::InvalidArguments)?, true),
        _ => return Err(Error::InvalidArguments),
    };
    if unit == operation.unit_id {
        return Err(Error::InvalidArguments);
    }
    let cycle:bool=sqlx::query_scalar("WITH RECURSIVE successors(unit_id) AS (SELECT $3::uuid UNION SELECT s.successor_unit_id FROM knowledge_supersessions s JOIN successors p ON p.unit_id=s.predecessor_unit_id WHERE s.tenant_id=$1 AND s.workspace_id=$2) SELECT EXISTS(SELECT 1 FROM successors WHERE unit_id=$4)")
        .bind(tenant).bind(workspace).bind(unit).bind(operation.unit_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if cycle {
        return Err(Error::InputConflict);
    }
    if same_change {
        let row:Option<(i64,Uuid,serde_json::Value)>=sqlx::query_as("SELECT h.accepted_revision,h.last_event_id,r.document_payload FROM knowledge_unit_heads h JOIN knowledge_revisions r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id AND r.unit_id=h.unit_id AND r.revision=h.accepted_revision WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.unit_id=$3 AND h.lifecycle='active' AND h.contract_version='dk-2' AND NOT h.payload_erased AND NOT r.payload_erased")
            .bind(tenant).bind(workspace).bind(unit).fetch_optional(&mut **tx).await.map_err(storage_error)?;
        let (revision, event, document) = row.ok_or(Error::NeedsContext)?;
        let document: KnowledgeDocumentDraft = decode(document)?;
        let valid:bool=sqlx::query_scalar("SELECT ($1::timestamptz IS NULL OR $1::timestamptz<=pg_catalog.clock_timestamp()) AND ($2::timestamptz IS NULL OR $2::timestamptz>=pg_catalog.clock_timestamp())")
            .bind(&document.valid_from).bind(&document.valid_until).fetch_one(&mut **tx).await.map_err(storage_error)?;
        let verified = event::verify_native_publication_event(
            tx, tenant, workspace, unit, revision, event, true,
        )
        .await?;
        if !valid || verified.input.planned.document.as_ref() != Some(&document) {
            return Err(Error::NeedsContext);
        }
    } else {
        context::eligible_unit(tx, tenant, workspace, principal, unit, None).await?;
    }
    Ok(Some(unit))
}
