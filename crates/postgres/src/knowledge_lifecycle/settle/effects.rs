use super::super::*;
use std::collections::BTreeSet;

pub(super) fn exact_effect_set(
    receipt: &KnowledgePublisherReceipt,
    rows: &[(Uuid, String, String, i64, String, String)],
) -> Result<bool> {
    let expected = receipt
        .effects
        .iter()
        .map(|value| Ok((value.effect_id, enum_text(&value.kind)?)))
        .collect::<Result<BTreeSet<_>>>()?;
    let actual = rows
        .iter()
        .map(|row| (row.0, row.1.clone()))
        .collect::<BTreeSet<_>>();
    let kinds = actual
        .iter()
        .map(|(_, kind)| kind.as_str())
        .collect::<BTreeSet<_>>();
    Ok(expected == actual
        && kinds
            == BTreeSet::from([
                "backup_disposition",
                "exact_delivery",
                "impact",
                "invalidation",
                "owned_copy_purge",
                "search",
                "visibility_closure",
            ]))
}

pub(super) fn rank(kind: KnowledgeEffectKind) -> u8 {
    match kind {
        KnowledgeEffectKind::OwnedCopyPurge => 0,
        KnowledgeEffectKind::ExactDelivery => 1,
        KnowledgeEffectKind::Invalidation => 2,
        KnowledgeEffectKind::Impact => 3,
        KnowledgeEffectKind::Search => 4,
        KnowledgeEffectKind::VisibilityClosure => 5,
        KnowledgeEffectKind::BackupDisposition => 6,
    }
}

fn erase_units(receipt: &KnowledgePublisherReceipt) -> Vec<Uuid> {
    receipt
        .applied_operations
        .iter()
        .filter(|value| value.operation == KnowledgeLifecycleOperation::Erase)
        .map(|value| value.unit_id)
        .collect()
}

async fn exact_delivery(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    receipt: &KnowledgePublisherReceipt,
) -> Result<bool> {
    for applied in &receipt.applied_operations {
        if applied.operation == KnowledgeLifecycleOperation::Erase {
            let erased: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM knowledge_suppression_ledger WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND change_id=$4 AND run_id=$5 AND event_id=$6 AND lifecycle='erased')",
            ).bind(tenant).bind(workspace).bind(applied.unit_id).bind(receipt.change_id)
                .bind(receipt.run_id).bind(applied.event_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
            if !erased
                || !erase::residual_owned_unit(tx, tenant, workspace, applied.unit_id)
                    .await?
                    .complete
            {
                return Ok(false);
            }
            continue;
        }
        if !intact_operation(tx, tenant, workspace, applied).await? {
            return Ok(false);
        }
    }
    Ok(!receipt.applied_operations.is_empty())
}

pub(super) async fn intact_operation(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    applied: &KnowledgeAppliedOperationReceipt,
) -> Result<bool> {
    if applied.operation == KnowledgeLifecycleOperation::Erase {
        return Ok(false);
    }
    let revision = applied.revision.ok_or(Error::InternalInvariant)?;
    let verified = event::verify_publication_event(
        tx,
        tenant,
        workspace,
        applied.unit_id,
        revision,
        applied.event_id,
        matches!(
            applied.operation,
            KnowledgeLifecycleOperation::Create | KnowledgeLifecycleOperation::Revise
        ),
    )
    .await?;
    if verified.rdf_digest != applied.rdf_digest || applied.rdf_digest_method != "rdfc-1.0-sha256" {
        return Ok(false);
    }
    match applied.operation {
        KnowledgeLifecycleOperation::Create | KnowledgeLifecycleOperation::Revise => sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND accepted_revision=$4 AND last_event_id=$5 AND lifecycle='active' AND active AND NOT payload_erased)",
        ).bind(tenant).bind(workspace).bind(applied.unit_id).bind(revision).bind(applied.event_id)
            .fetch_one(&mut **tx).await.map_err(storage_error),
        KnowledgeLifecycleOperation::Revalidate => sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND accepted_revision=$4 AND last_event_id=$5 AND last_validation_event_id=$5 AND NOT payload_erased)",
        ).bind(tenant).bind(workspace).bind(applied.unit_id).bind(revision).bind(applied.event_id)
            .fetch_one(&mut **tx).await.map_err(storage_error),
        KnowledgeLifecycleOperation::Supersede => sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM knowledge_supersessions WHERE tenant_id=$1 AND workspace_id=$2 AND predecessor_unit_id=$3 AND event_id=$4)",
        ).bind(tenant).bind(workspace).bind(applied.unit_id).bind(applied.event_id)
            .fetch_one(&mut **tx).await.map_err(storage_error),
        KnowledgeLifecycleOperation::Retract => retracted(tx, tenant, workspace, applied.unit_id).await,
        KnowledgeLifecycleOperation::Erase => unreachable!(),
    }
}

async fn retracted(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
) -> Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM knowledge_unit_heads h WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.unit_id=$3 AND h.lifecycle='retracted' AND NOT h.active AND NOT EXISTS(SELECT 1 FROM knowledge_bindings b WHERE b.tenant_id=h.tenant_id AND b.workspace_id=h.workspace_id AND b.unit_id=h.unit_id AND b.active))",
    ).bind(tenant).bind(workspace).bind(unit).fetch_one(&mut **tx).await.map_err(storage_error)
}

async fn invalidated(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    receipt: &KnowledgePublisherReceipt,
) -> Result<bool> {
    let generation: i64 = sqlx::query_scalar(
        "SELECT generation FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(!receipt.applied_operations.is_empty()
        && receipt.workspace_generation > 0
        && generation >= receipt.workspace_generation)
}

async fn impact_reviewed(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    receipt: &KnowledgePublisherReceipt,
) -> Result<bool> {
    let impact: KnowledgeImpactPlan = phase_data::load_phase_data(
        tx,
        tenant,
        workspace,
        receipt.run_id,
        KnowledgeChangePhaseId::KcImpactPlan,
    )
    .await?;
    let review: KnowledgeReviewReceipt = phase_data::load_phase_data(
        tx,
        tenant,
        workspace,
        receipt.run_id,
        KnowledgeChangePhaseId::KcReviewReconcile,
    )
    .await?;
    let applied = receipt
        .applied_operations
        .iter()
        .map(|v| v.operation_id)
        .collect::<BTreeSet<_>>();
    let covered = review
        .covered_operation_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    Ok(review.outcome == KnowledgeReviewOutcome::Ready
        && applied == covered
        && review
            .reviewed_digests
            .iter()
            .any(|value| value == &impact.digest)
        && impact.blocking_conflicts.is_empty()
        && review.findings.iter().all(|value| value.closed))
}

async fn visibility_closed(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    receipt: &KnowledgePublisherReceipt,
) -> Result<bool> {
    for applied in &receipt.applied_operations {
        match applied.operation {
            KnowledgeLifecycleOperation::Retract => {
                if !retracted(tx, tenant, workspace, applied.unit_id).await? {
                    return Ok(false);
                }
            }
            KnowledgeLifecycleOperation::Erase => {
                let head: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM knowledge_unit_heads h WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.unit_id=$3 AND h.lifecycle='erased' AND NOT h.active AND h.payload_erased AND NOT EXISTS(SELECT 1 FROM knowledge_bindings b WHERE b.tenant_id=h.tenant_id AND b.workspace_id=h.workspace_id AND b.unit_id=h.unit_id AND b.active))",
                ).bind(tenant).bind(workspace).bind(applied.unit_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
                if !head
                    || !erase::residual_owned_unit(tx, tenant, workspace, applied.unit_id)
                        .await?
                        .complete
                {
                    return Ok(false);
                }
            }
            _ => {}
        }
    }
    Ok(true)
}

async fn purge_owned(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    receipt: &KnowledgePublisherReceipt,
) -> Result<bool> {
    let units = erase_units(receipt);
    if units.is_empty() {
        return Ok(false);
    }
    for unit in units {
        if erase::suppress_owned_unit(tx, tenant, workspace, unit)
            .await?
            .remaining
            != 0
            || !erase::residual_owned_unit(tx, tenant, workspace, unit)
                .await?
                .complete
        {
            return Ok(false);
        }
        sqlx::query("UPDATE knowledge_suppression_ledger SET lifecycle='erased',owned_live_copies_status='ready' WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
            .bind(tenant).bind(workspace).bind(unit).execute(&mut **tx).await.map_err(storage_error)?;
    }
    Ok(true)
}

async fn restore_safe(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    receipt: &KnowledgePublisherReceipt,
) -> Result<(bool, i64)> {
    let units = erase_units(receipt);
    if units.is_empty() {
        return Ok((false, 0));
    }
    let sequences: Vec<i64> = sqlx::query_scalar(
        "SELECT erasure_sequence FROM knowledge_suppression_ledger WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3 AND run_id=$4 AND unit_id=ANY($5) ORDER BY erasure_sequence",
    ).bind(tenant).bind(workspace).bind(receipt.change_id).bind(receipt.run_id).bind(&units)
        .fetch_all(&mut **tx).await.map_err(storage_error)?;
    if sequences.len() != units.len() {
        return Ok((false, 0));
    }
    let required = *sequences.last().ok_or(Error::InternalInvariant)?;
    let covered: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM knowledge_suppression_exports e JOIN durable_knowledge_capability c ON c.singleton AND c.database_lineage_id=e.database_lineage_id WHERE e.erasure_sequence >= $1 AND e.manifest_digest<>'')",
    ).bind(required).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if covered {
        sqlx::query("UPDATE knowledge_suppression_ledger SET restore_safe_status='ready' WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=ANY($3) AND erasure_sequence<=$4")
            .bind(tenant).bind(workspace).bind(&units).bind(required).execute(&mut **tx).await.map_err(storage_error)?;
    }
    Ok((covered, required))
}

pub(super) async fn refresh(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    effect: &mut KnowledgeEffectReceipt,
    receipt: &KnowledgePublisherReceipt,
    completion: &KnowledgeCompletionRequirement,
) -> Result<()> {
    let closure = receipt.applied_operations.iter().any(|v| {
        matches!(
            v.operation,
            KnowledgeLifecycleOperation::Retract | KnowledgeLifecycleOperation::Erase
        )
    });
    let erase = !erase_units(receipt).is_empty();
    let (status, detail) = match effect.kind {
        KnowledgeEffectKind::ExactDelivery if !completion.exact_delivery => (
            KnowledgeEffectStatus::NotApplicable,
            "exact delivery was not required".into(),
        ),
        KnowledgeEffectKind::ExactDelivery => (
            ready(exact_delivery(tx, tenant, workspace, receipt).await?),
            "typed publication or expected withdrawal checked".into(),
        ),
        KnowledgeEffectKind::Invalidation => (
            ready(invalidated(tx, tenant, workspace, receipt).await?),
            format!(
                "workspace generation {} is the context basis",
                receipt.workspace_generation
            ),
        ),
        KnowledgeEffectKind::Impact if !completion.impact_recorded => (
            KnowledgeEffectStatus::NotApplicable,
            "reviewed impact was not required".into(),
        ),
        KnowledgeEffectKind::Impact => (
            ready(impact_reviewed(tx, tenant, workspace, receipt).await?),
            "stored impact, followups, and review coverage checked".into(),
        ),
        KnowledgeEffectKind::Search => (
            KnowledgeEffectStatus::NotConfigured,
            "search is not configured".into(),
        ),
        KnowledgeEffectKind::VisibilityClosure if !closure => (
            KnowledgeEffectStatus::NotApplicable,
            "no retraction or erasure visibility closure applies".into(),
        ),
        KnowledgeEffectKind::VisibilityClosure => (
            ready(visibility_closed(tx, tenant, workspace, receipt).await?),
            "closed unit reads and active bindings checked".into(),
        ),
        KnowledgeEffectKind::OwnedCopyPurge if !erase => (
            KnowledgeEffectStatus::NotApplicable,
            "no erasure-owned copy purge applies".into(),
        ),
        KnowledgeEffectKind::OwnedCopyPurge => (
            ready(purge_owned(tx, tenant, workspace, receipt).await?),
            "receipt-scoped owned live copies and residual checked".into(),
        ),
        KnowledgeEffectKind::BackupDisposition
            if completion.erasure != KnowledgeErasureRequirement::RestoreSafe =>
        {
            (
                KnowledgeEffectStatus::NotApplicable,
                "restore-safe disposition was not required".into(),
            )
        }
        KnowledgeEffectKind::BackupDisposition => {
            let (covered, sequence) = restore_safe(tx, tenant, workspace, receipt).await?;
            let detail = if covered {
                format!("recorded export covers receipt erasure sequence {sequence}")
            } else {
                format!("operator checkpoint required through erasure sequence {sequence}")
            };
            (ready(covered), detail)
        }
    };
    effect.status = status;
    effect.detail = detail;
    sqlx::query("UPDATE knowledge_lifecycle_effects SET status=$4,detail=$5,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(effect.effect_id).bind(enum_text(&effect.status)?).bind(&effect.detail)
        .execute(&mut **tx).await.map_err(storage_error)?;
    Ok(())
}

fn ready(value: bool) -> KnowledgeEffectStatus {
    if value {
        KnowledgeEffectStatus::Ready
    } else {
        KnowledgeEffectStatus::Pending
    }
}

pub(super) fn required_complete(
    completion: &KnowledgeCompletionRequirement,
    effects: &[KnowledgeEffectReceipt],
) -> bool {
    let ready = |kind| {
        effects.iter().any(|v| {
            v.kind == kind
                && matches!(
                    v.status,
                    KnowledgeEffectStatus::Ready | KnowledgeEffectStatus::NotApplicable
                )
        })
    };
    (!completion.exact_delivery || ready(KnowledgeEffectKind::ExactDelivery))
        && ready(KnowledgeEffectKind::Invalidation)
        && (!completion.impact_recorded || ready(KnowledgeEffectKind::Impact))
        && completion.search == KnowledgeSearchRequirement::NotRequired
        && ready(KnowledgeEffectKind::VisibilityClosure)
        && match completion.erasure {
            KnowledgeErasureRequirement::NotRequired => true,
            KnowledgeErasureRequirement::OwnedLiveCopies => {
                ready(KnowledgeEffectKind::OwnedCopyPurge)
            }
            KnowledgeErasureRequirement::RestoreSafe => {
                ready(KnowledgeEffectKind::OwnedCopyPurge)
                    && ready(KnowledgeEffectKind::BackupDisposition)
            }
            KnowledgeErasureRequirement::AllRetainedCopies => false,
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effect(kind: KnowledgeEffectKind, status: KnowledgeEffectStatus) -> KnowledgeEffectReceipt {
        KnowledgeEffectReceipt {
            effect_id: Uuid::new_v4(),
            kind,
            status,
            generation: 1,
            owner_ref: "backend".into(),
            detail: String::new(),
        }
    }

    #[test]
    fn selected_effects_cannot_hide_a_required_pending_effect() {
        let completion = KnowledgeCompletionRequirement {
            canonical_result: true,
            exact_delivery: true,
            impact_recorded: true,
            search: KnowledgeSearchRequirement::NotRequired,
            erasure: KnowledgeErasureRequirement::RestoreSafe,
        };
        let mut effects = vec![
            effect(
                KnowledgeEffectKind::ExactDelivery,
                KnowledgeEffectStatus::Ready,
            ),
            effect(
                KnowledgeEffectKind::Invalidation,
                KnowledgeEffectStatus::Ready,
            ),
            effect(KnowledgeEffectKind::Impact, KnowledgeEffectStatus::Ready),
            effect(
                KnowledgeEffectKind::Search,
                KnowledgeEffectStatus::NotConfigured,
            ),
            effect(
                KnowledgeEffectKind::VisibilityClosure,
                KnowledgeEffectStatus::Ready,
            ),
            effect(
                KnowledgeEffectKind::OwnedCopyPurge,
                KnowledgeEffectStatus::Ready,
            ),
            effect(
                KnowledgeEffectKind::BackupDisposition,
                KnowledgeEffectStatus::Pending,
            ),
        ];
        assert!(!required_complete(&completion, &effects));
        effects.last_mut().unwrap().status = KnowledgeEffectStatus::Ready;
        assert!(required_complete(&completion, &effects));
    }
}
