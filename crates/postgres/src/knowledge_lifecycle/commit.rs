mod apply;
mod binding;

use apply::apply_operation;
use binding::insert_bindings;

use super::*;
use std::collections::{BTreeMap, BTreeSet};

fn replay_outcome(value: CommitKnowledgeChangeOutcome) -> CommitKnowledgeChangeOutcome {
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

fn topo(mut values: Vec<KnowledgePlannedOperation>) -> Result<Vec<KnowledgePlannedOperation>> {
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

async fn lock_head(
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

async fn resolved_sources(
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

async fn require_new_revalidation_evidence(
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

async fn successor(
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

#[allow(clippy::too_many_arguments)]
pub(crate) async fn commit(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Uuid,
    request: &CommitKnowledgeChange,
) -> Result<CommitKnowledgeChangeOutcome> {
    require_owner(tx, principal).await?;
    let payload = json(request)?;
    if let Some(prior) = replay::<CommitKnowledgeChangeOutcome>(
        tx,
        tenant,
        workspace,
        principal,
        "commit",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(replay_outcome(prior));
    }
    crate::durable_knowledge::publisher_gate(tx).await?;
    let generation = lock_workspace(tx, tenant, workspace).await?;
    let (revision, status, current) =
        lock_run(tx, tenant, workspace, request.change_id, request.run_id).await?;
    if let Some(prior) = replay::<CommitKnowledgeChangeOutcome>(
        tx,
        tenant,
        workspace,
        principal,
        "commit",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(replay_outcome(prior));
    }
    if revision != request.run_revision
        || status != "active"
        || current.as_deref() != Some(KnowledgeChangePhaseId::KcCommit.as_str())
    {
        return Err(Error::StaleRevision);
    }
    let ready:KnowledgeReadyToCommit=decode(sqlx::query_scalar::<_,Option<serde_json::Value>>("SELECT ready_to_commit FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(request.run_id).fetch_one(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NeedsContext)?)?;
    if ready.seal_id != request.seal_id
        || ready.plan_revision != request.plan_revision
        || ready.plan_digest != request.plan_digest
        || ready.command_digest != request.sealed_command_digest
        || ready.run_revision != revision
        || ready.workspace_generation != generation
    {
        return Err(Error::StaleContext);
    }
    let completion:KnowledgeCompletionRequirement=decode(sqlx::query_scalar("SELECT completion FROM knowledge_lifecycle_changes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(request.change_id).fetch_one(&mut **tx).await.map_err(storage_error)?)?;
    if completion.search == KnowledgeSearchRequirement::Required {
        return Err(Error::KnowledgeUnavailable);
    }
    if completion.erasure == KnowledgeErasureRequirement::AllRetainedCopies {
        return Err(Error::UnsupportedCompletionRequirement);
    }
    erase::reconcile_change_owned_copies(tx, tenant, workspace, request.change_id).await?;
    let sealed = commit_guard::verify(
        tx,
        tenant,
        workspace,
        request.change_id,
        request.run_id,
        revision,
        generation,
        &ready,
    )
    .await?;
    let ordered = topo(sealed.changeset.operations)?;
    let mut created = BTreeMap::new();
    let mut applied = Vec::new();
    for operation in &ordered {
        let receipt = apply_operation(
            tx,
            tenant,
            workspace,
            request.change_id,
            principal,
            session,
            operation,
            &created,
        )
        .await?;
        if operation.operation == KnowledgeLifecycleOperation::Create {
            created.insert(operation.operation_id, operation.unit_id);
        }
        applied.push(receipt);
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
    let has_erase = ordered
        .iter()
        .any(|value| value.operation == KnowledgeLifecycleOperation::Erase);
    let has_visibility_close = ordered.iter().any(|value| {
        matches!(
            value.operation,
            KnowledgeLifecycleOperation::Retract | KnowledgeLifecycleOperation::Erase
        )
    });
    let mut exact_delivery_ready = !has_erase;
    for applied_operation in &applied {
        if applied_operation.operation == KnowledgeLifecycleOperation::Erase {
            continue;
        }
        let include_revision = matches!(
            applied_operation.operation,
            KnowledgeLifecycleOperation::Create | KnowledgeLifecycleOperation::Revise
        );
        let verified = event::verify_native_publication_event(
            tx,
            tenant,
            workspace,
            applied_operation.unit_id,
            applied_operation.revision.ok_or(Error::InternalInvariant)?,
            applied_operation.event_id,
            include_revision,
        )
        .await?;
        if verified.rdf_digest != applied_operation.rdf_digest {
            exact_delivery_ready = false;
        }
    }
    let visibility_ready = has_visibility_close
        && !has_erase
        && sqlx::query_scalar::<_, bool>(
            "SELECT NOT EXISTS(SELECT 1 FROM knowledge_change_operations o JOIN knowledge_unit_heads h ON h.tenant_id=o.tenant_id AND h.workspace_id=o.workspace_id AND h.unit_id=o.unit_id WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.change_id=$3 AND o.operation='retract' AND (h.lifecycle<>'retracted' OR EXISTS(SELECT 1 FROM knowledge_bindings b WHERE b.tenant_id=h.tenant_id AND b.workspace_id=h.workspace_id AND b.unit_id=h.unit_id AND b.active)))",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(request.change_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    let receipt_id = Uuid::new_v4();
    let mut effects = Vec::new();
    for kind in [
        KnowledgeEffectKind::ExactDelivery,
        KnowledgeEffectKind::Invalidation,
        KnowledgeEffectKind::Impact,
        KnowledgeEffectKind::Search,
        KnowledgeEffectKind::VisibilityClosure,
        KnowledgeEffectKind::OwnedCopyPurge,
        KnowledgeEffectKind::BackupDisposition,
    ] {
        let status = match kind {
            KnowledgeEffectKind::ExactDelivery if exact_delivery_ready => {
                KnowledgeEffectStatus::Ready
            }
            KnowledgeEffectKind::Invalidation | KnowledgeEffectKind::Impact => {
                KnowledgeEffectStatus::Ready
            }
            KnowledgeEffectKind::Search => KnowledgeEffectStatus::NotConfigured,
            KnowledgeEffectKind::VisibilityClosure if visibility_ready => {
                KnowledgeEffectStatus::Ready
            }
            KnowledgeEffectKind::VisibilityClosure if !has_visibility_close => {
                KnowledgeEffectStatus::NotApplicable
            }
            KnowledgeEffectKind::OwnedCopyPurge if !has_erase => {
                KnowledgeEffectStatus::NotApplicable
            }
            KnowledgeEffectKind::BackupDisposition
                if completion.erasure != KnowledgeErasureRequirement::RestoreSafe =>
            {
                KnowledgeEffectStatus::NotApplicable
            }
            _ => KnowledgeEffectStatus::Pending,
        };
        let effect = KnowledgeEffectReceipt {
            effect_id: Uuid::new_v4(),
            kind,
            status,
            generation: next_generation,
            owner_ref: "tect-backend".into(),
            detail: "backend recorded canonical effect".into(),
        };
        sqlx::query("INSERT INTO knowledge_lifecycle_effects(id,tenant_id,workspace_id,change_id,publisher_receipt_id,kind,status,generation,owner_ref,detail) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)").bind(effect.effect_id).bind(tenant).bind(workspace).bind(request.change_id).bind(receipt_id).bind(enum_text(&kind)?).bind(enum_text(&status)?).bind(next_generation).bind(&effect.owner_ref).bind(&effect.detail).execute(&mut **tx).await.map_err(storage_error)?;
        effects.push(effect);
    }
    let mut receipt = KnowledgePublisherReceipt {
        id: receipt_id,
        request_id: request.request_id,
        change_id: request.change_id,
        run_id: request.run_id,
        sealed_command_digest: request.sealed_command_digest.clone(),
        workspace_generation: next_generation,
        applied_operations: applied,
        effects,
        digest: String::new(),
    };
    receipt.digest = digest(&receipt)?;
    sqlx::query("UPDATE knowledge_change_runs SET revision=revision+1,publisher_receipt=$4,current_phase_id='kc-settle-effects',current_phase_ordinal=11,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(request.run_id).bind(json(&receipt)?).execute(&mut **tx).await.map_err(storage_error)?;
    let result = CommitKnowledgeChangeOutcome::Applied(receipt);
    save_receipt(
        tx,
        tenant,
        workspace,
        principal,
        session,
        "commit",
        request.request_id,
        &payload,
        &result,
    )
    .await?;
    let erased_units = ordered
        .iter()
        .filter(|operation| operation.operation == KnowledgeLifecycleOperation::Erase)
        .map(|operation| operation.unit_id)
        .collect::<Vec<_>>();
    if erased_units.is_empty() {
        return Ok(result);
    }
    erase::reconcile_change_owned_copies(tx, tenant, workspace, request.change_id).await?;
    for unit in erased_units {
        let report = erase::suppress_owned_unit(tx, tenant, workspace, unit).await?;
        let complete = report.remaining == 0;
        sqlx::query("UPDATE knowledge_unit_heads SET lifecycle=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
            .bind(tenant).bind(workspace).bind(unit).bind(if complete {"erased"} else {"erasure_pending"}).execute(&mut **tx).await.map_err(storage_error)?;
        sqlx::query("UPDATE knowledge_suppression_ledger SET lifecycle=$4,owned_live_copies_status=$5 WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
            .bind(tenant).bind(workspace).bind(unit).bind(if complete {"erased"} else {"erasure_pending"}).bind(if complete {"ready"} else {"pending"}).execute(&mut **tx).await.map_err(storage_error)?;
    }
    let erased:KnowledgeErasedPublisherReceipt=decode(sqlx::query_scalar::<_,Option<serde_json::Value>>("SELECT erased_publisher_receipt FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(request.run_id).fetch_one(&mut **tx).await.map_err(storage_error)?.ok_or(Error::InternalInvariant)?)?;
    Ok(CommitKnowledgeChangeOutcome::AppliedErased(erased))
}
