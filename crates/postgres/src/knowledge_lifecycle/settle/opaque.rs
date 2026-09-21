use super::super::*;
use std::collections::BTreeSet;

pub(super) async fn load_report(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    projection: KnowledgeErasedEffectsReport,
) -> Result<KnowledgeEffectsReport> {
    let rows: Vec<(Uuid, String, String, i64, String, String)> = sqlx::query_as(
        "SELECT id,kind,status,generation,owner_ref,detail FROM knowledge_lifecycle_effects WHERE tenant_id=$1 AND workspace_id=$2 AND publisher_receipt_id=$3 ORDER BY id",
    )
    .bind(tenant).bind(workspace).bind(projection.publisher_receipt_id)
    .fetch_all(&mut **tx).await.map_err(storage_error)?;
    if rows.len() != projection.effects.len() {
        return Err(Error::InternalInvariant);
    }
    let mut effects = Vec::with_capacity(rows.len());
    for opaque in &projection.effects {
        let row = rows
            .iter()
            .find(|row| row.0 == opaque.effect_id)
            .ok_or(Error::InternalInvariant)?;
        let kind: KnowledgeEffectKind = decode(serde_json::Value::String(row.1.clone()))?;
        let status = decode_effect_status(&row.2)?;
        if kind != opaque.kind || status != opaque.status || row.3 != opaque.generation {
            return Err(Error::InternalInvariant);
        }
        effects.push(KnowledgeEffectReceipt {
            effect_id: row.0,
            kind,
            status,
            generation: row.3,
            owner_ref: row.4.clone(),
            detail: row.5.clone(),
        });
    }
    let remaining_work = effects
        .iter()
        .filter(|value| {
            matches!(
                value.status,
                KnowledgeEffectStatus::Pending | KnowledgeEffectStatus::Failed
            )
        })
        .map(|value| format!("{:?}", value.kind).to_lowercase())
        .collect();
    Ok(KnowledgeEffectsReport {
        publisher_receipt_id: projection.publisher_receipt_id,
        effects,
        required_complete: projection.required_complete,
        remaining_work,
    })
}

fn decode_effect_status(raw: &str) -> Result<KnowledgeEffectStatus> {
    decode(serde_json::Value::String(raw.to_owned())).map_err(|_| {
        Error::refused(
            RefusalCode::EffectStatusUnknown,
            "inspect_effect_status",
            "known_effect_status",
        )
    })
}

#[cfg(test)]
mod refusal_tests {
    use super::*;

    #[test]
    fn unknown_effect_status_has_typed_refusal() {
        let error = decode_effect_status("not-a-status").unwrap_err();
        assert_eq!(
            error.refusal().unwrap().code,
            RefusalCode::EffectStatusUnknown
        );
    }
}

pub(super) fn exact_effect_set(
    receipt: &KnowledgeErasedPublisherReceipt,
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

fn erased_units(receipt: &KnowledgeErasedPublisherReceipt) -> Vec<Uuid> {
    receipt
        .operations
        .iter()
        .filter_map(|value| match value {
            KnowledgeRetainedOperationReceipt::PayloadErased(value) => Some(value.unit_id),
            KnowledgeRetainedOperationReceipt::Intact(_) => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

async fn erased_unit_closed(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    receipt: &KnowledgeErasedPublisherReceipt,
    value: &KnowledgeErasedOperationReceipt,
) -> Result<bool> {
    let ledger: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM knowledge_suppression_ledger WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND change_id=$4 AND run_id=$5 AND erasure_sequence=$6 AND lifecycle='erased')",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(value.unit_id)
    .bind(receipt.change_id)
    .bind(receipt.run_id)
    .bind(value.erasure_sequence)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let head: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM knowledge_unit_heads h WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.unit_id=$3 AND h.lifecycle='erased' AND NOT h.active AND h.payload_erased AND NOT EXISTS(SELECT 1 FROM knowledge_bindings b WHERE b.tenant_id=h.tenant_id AND b.workspace_id=h.workspace_id AND b.unit_id=h.unit_id AND b.active))",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(value.unit_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(ledger
        && head
        && erase::residual_owned_unit(tx, tenant, workspace, value.unit_id)
            .await?
            .complete)
}

async fn exact_delivery(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    receipt: &KnowledgeErasedPublisherReceipt,
) -> Result<bool> {
    for retained in &receipt.operations {
        match retained {
            KnowledgeRetainedOperationReceipt::PayloadErased(value) => {
                if !erased_unit_closed(tx, tenant, workspace, receipt, value).await? {
                    return Ok(false);
                }
                if value.operation == KnowledgeLifecycleOperation::Erase {
                    let event_matches: bool = sqlx::query_scalar(
                        "SELECT EXISTS(SELECT 1 FROM knowledge_suppression_ledger WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND event_id=$4 AND erasure_sequence=$5)",
                    )
                    .bind(tenant)
                    .bind(workspace)
                    .bind(value.unit_id)
                    .bind(value.event_id)
                    .bind(value.erasure_sequence)
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(storage_error)?;
                    if !event_matches {
                        return Ok(false);
                    }
                }
            }
            KnowledgeRetainedOperationReceipt::Intact(value) => {
                if !super::effects::intact_operation(tx, tenant, workspace, value).await? {
                    return Ok(false);
                }
            }
        }
    }
    Ok(!receipt.operations.is_empty())
}

async fn purge(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    receipt: &KnowledgeErasedPublisherReceipt,
) -> Result<bool> {
    let units = erased_units(receipt);
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
    receipt: &KnowledgeErasedPublisherReceipt,
) -> Result<(bool, i64)> {
    let units = erased_units(receipt);
    let required = receipt
        .operations
        .iter()
        .filter_map(|value| match value {
            KnowledgeRetainedOperationReceipt::PayloadErased(value) => Some(value.erasure_sequence),
            KnowledgeRetainedOperationReceipt::Intact(_) => None,
        })
        .max()
        .unwrap_or(0);
    if units.is_empty() || required == 0 {
        return Ok((false, required));
    }
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
    receipt: &KnowledgeErasedPublisherReceipt,
) -> Result<()> {
    let retained = receipt
        .effects
        .iter()
        .find(|value| value.effect_id == effect.effect_id && value.kind == effect.kind)
        .ok_or(Error::InternalInvariant)?;
    let has_erased = !erased_units(receipt).is_empty();
    let has_retract = receipt.operations.iter().any(|value| matches!(value, KnowledgeRetainedOperationReceipt::Intact(value) if value.operation == KnowledgeLifecycleOperation::Retract));
    let (status, detail) = match effect.kind {
        KnowledgeEffectKind::ExactDelivery if !receipt.completion.exact_delivery => (
            KnowledgeEffectStatus::NotApplicable,
            "exact delivery was not required".into(),
        ),
        KnowledgeEffectKind::ExactDelivery => (
            ready(exact_delivery(tx, tenant, workspace, receipt).await?),
            "retained typed survivors and expected withdrawals checked".into(),
        ),
        KnowledgeEffectKind::Invalidation => {
            let generation: i64 = sqlx::query_scalar("SELECT generation FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2")
                .bind(tenant).bind(workspace).fetch_one(&mut **tx).await.map_err(storage_error)?;
            (
                ready(
                    retained.status == KnowledgeEffectStatus::Ready
                        && generation >= retained.generation,
                ),
                format!(
                    "retained generation {} and current context basis checked",
                    retained.generation
                ),
            )
        }
        KnowledgeEffectKind::Impact if !receipt.completion.impact_recorded => (
            KnowledgeEffectStatus::NotApplicable,
            "reviewed impact was not required".into(),
        ),
        KnowledgeEffectKind::Impact => (
            ready(retained.status == KnowledgeEffectStatus::Ready),
            "transactional pre-scrub impact and followup attestation retained".into(),
        ),
        KnowledgeEffectKind::Search
            if receipt.completion.search == KnowledgeSearchRequirement::NotRequired =>
        {
            (
                KnowledgeEffectStatus::NotApplicable,
                "search completion was not required".into(),
            )
        }
        KnowledgeEffectKind::Search => {
            let units = receipt
                .operations
                .iter()
                .map(|value| match value {
                    KnowledgeRetainedOperationReceipt::PayloadErased(value) => value.unit_id,
                    KnowledgeRetainedOperationReceipt::Intact(value) => value.unit_id,
                })
                .collect::<Vec<_>>();
            (
                crate::knowledge_search::search_effect_status(tx, tenant, workspace, &units, true)
                    .await?,
                "current canonical search projection checked".into(),
            )
        }
        KnowledgeEffectKind::VisibilityClosure if !has_erased && !has_retract => (
            KnowledgeEffectStatus::NotApplicable,
            "no retraction or erasure visibility closure applies".into(),
        ),
        KnowledgeEffectKind::VisibilityClosure => {
            let mut closed = true;
            for value in &receipt.operations {
                match value {
                    KnowledgeRetainedOperationReceipt::PayloadErased(value) => {
                        closed &= erased_unit_closed(tx, tenant, workspace, receipt, value).await?
                    }
                    KnowledgeRetainedOperationReceipt::Intact(value)
                        if value.operation == KnowledgeLifecycleOperation::Retract =>
                    {
                        closed &= sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM knowledge_unit_heads h WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.unit_id=$3 AND h.lifecycle='retracted' AND NOT h.active AND NOT EXISTS(SELECT 1 FROM knowledge_bindings b WHERE b.tenant_id=h.tenant_id AND b.workspace_id=h.workspace_id AND b.unit_id=h.unit_id AND b.active))")
                            .bind(tenant).bind(workspace).bind(value.unit_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
                    }
                    _ => {}
                }
            }
            (
                ready(closed),
                "closed retained reads and active bindings checked".into(),
            )
        }
        KnowledgeEffectKind::OwnedCopyPurge if !has_erased => (
            KnowledgeEffectStatus::NotApplicable,
            "no erasure-owned copy purge applies".into(),
        ),
        KnowledgeEffectKind::OwnedCopyPurge => (
            ready(purge(tx, tenant, workspace, receipt).await?),
            "retained receipt units and residual checked".into(),
        ),
        KnowledgeEffectKind::BackupDisposition
            if receipt.completion.erasure != KnowledgeErasureRequirement::RestoreSafe =>
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
