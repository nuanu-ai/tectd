mod apply;
mod binding;
mod guard;

use apply::apply_operation;
use binding::insert_bindings;
use guard::*;

use super::*;
use std::collections::BTreeMap;

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
            KnowledgeEffectKind::Search
                if completion.search == KnowledgeSearchRequirement::NotRequired =>
            {
                KnowledgeEffectStatus::NotApplicable
            }
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
    // Exact reads used by the rebuildable search projection require an operation receipt.
    // This intermediate value is transaction-local and is replaced with the final effect state.
    sqlx::query("UPDATE knowledge_change_runs SET revision=revision+1,publisher_receipt=$4,current_phase_id='kc-settle-effects',current_phase_ordinal=11,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(request.run_id).bind(json(&receipt)?).execute(&mut **tx).await.map_err(storage_error)?;
    for operation in &ordered {
        crate::knowledge_search::apply_dk2_operation(
            tx,
            tenant,
            workspace,
            principal,
            operation.unit_id,
            next_generation,
        )
        .await?;
    }
    let units = ordered
        .iter()
        .map(|value| value.unit_id)
        .collect::<Vec<_>>();
    let search_status = crate::knowledge_search::search_effect_status(
        tx,
        tenant,
        workspace,
        &units,
        completion.search == KnowledgeSearchRequirement::Required,
    )
    .await?;
    let search = receipt
        .effects
        .iter_mut()
        .find(|value| value.kind == KnowledgeEffectKind::Search)
        .ok_or(Error::InternalInvariant)?;
    search.status = search_status;
    search.detail = if search_status == KnowledgeEffectStatus::NotApplicable {
        "search completion was not required".into()
    } else {
        "current canonical search projection checked".into()
    };
    sqlx::query("UPDATE knowledge_lifecycle_effects SET status=$4,detail=$5 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(search.effect_id).bind(enum_text(&search_status)?).bind(&search.detail)
        .execute(&mut **tx).await.map_err(storage_error)?;
    receipt.digest = String::new();
    receipt.digest = digest(&receipt)?;
    sqlx::query("UPDATE knowledge_change_runs SET publisher_receipt=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(request.run_id).bind(json(&receipt)?)
        .execute(&mut **tx).await.map_err(storage_error)?;
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
