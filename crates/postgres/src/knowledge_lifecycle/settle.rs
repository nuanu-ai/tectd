use super::*;
use std::collections::BTreeSet;

mod effects;
mod opaque;

enum ReceiptState {
    Intact(KnowledgePublisherReceipt),
    PayloadErased(KnowledgeErasedPublisherReceipt),
}

fn replay_outcome(
    value: SettleKnowledgeChangeEffectsOutcome,
) -> SettleKnowledgeChangeEffectsOutcome {
    match value {
        SettleKnowledgeChangeEffectsOutcome::Settled(report)
        | SettleKnowledgeChangeEffectsOutcome::Replay(report) => {
            SettleKnowledgeChangeEffectsOutcome::Replay(report)
        }
    }
}

pub(crate) async fn load_effects_report(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
) -> Result<Option<KnowledgeEffectsReport>> {
    let row: Option<(bool, Option<serde_json::Value>, Option<serde_json::Value>)> =
        sqlx::query_as("SELECT payload_erased,effects_report,erased_effects_report FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
            .bind(tenant).bind(workspace).bind(run).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    match row {
        None | Some((false, None, None)) | Some((true, None, None)) => Ok(None),
        Some((false, Some(value), None)) => Ok(Some(decode(value)?)),
        Some((true, None, Some(value))) => Ok(Some(
            opaque::load_report(tx, tenant, workspace, decode(value)?).await?,
        )),
        Some(_) => Err(Error::InternalInvariant),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn settle(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Uuid,
    request: &SettleKnowledgeChangeEffects,
) -> Result<SettleKnowledgeChangeEffectsOutcome> {
    require_owner(tx, principal).await?;
    let payload = json(request)?;
    if let Some(prior) = replay::<SettleKnowledgeChangeEffectsOutcome>(
        tx,
        tenant,
        workspace,
        principal,
        "settle_effects",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(replay_outcome(prior));
    }
    let _ = lock_workspace(tx, tenant, workspace).await?;
    let (revision, status, current) =
        lock_run(tx, tenant, workspace, request.change_id, request.run_id).await?;
    if let Some(prior) = replay::<SettleKnowledgeChangeEffectsOutcome>(
        tx,
        tenant,
        workspace,
        principal,
        "settle_effects",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(replay_outcome(prior));
    }
    if revision != request.run_revision
        || status != "active"
        || current.as_deref() != Some(KnowledgeChangePhaseId::KcSettleEffects.as_str())
    {
        return Err(Error::StaleRevision);
    }
    let stored: (Option<serde_json::Value>, Option<serde_json::Value>) = sqlx::query_as(
        "SELECT publisher_receipt,erased_publisher_receipt FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.run_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let receipt = match stored {
        (Some(value), None) => ReceiptState::Intact(decode(value)?),
        (None, Some(value)) => ReceiptState::PayloadErased(decode(value)?),
        _ => return Err(Error::InternalInvariant),
    };
    let receipt_id = match &receipt {
        ReceiptState::Intact(value) => value.id,
        ReceiptState::PayloadErased(value) => value.id,
    };
    if receipt_id != request.publisher_receipt_id {
        return Err(Error::InputConflict);
    }
    let completion = match &receipt {
        ReceiptState::Intact(_) => decode(sqlx::query_scalar("SELECT completion FROM knowledge_lifecycle_changes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(request.change_id).fetch_one(&mut **tx).await.map_err(storage_error)?)?,
        ReceiptState::PayloadErased(value) => {
            if value.change_id != request.change_id || value.run_id != request.run_id {
                return Err(Error::InternalInvariant);
            }
            value.completion.clone()
        }
    };
    let selected = request.effect_ids.iter().copied().collect::<BTreeSet<_>>();
    let rows:Vec<(Uuid,String,String,i64,String,String)>=sqlx::query_as("SELECT id,kind,status,generation,owner_ref,detail FROM knowledge_lifecycle_effects WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3 AND publisher_receipt_id=$4 ORDER BY id").bind(tenant).bind(workspace).bind(request.change_id).bind(receipt_id).fetch_all(&mut **tx).await.map_err(storage_error)?;
    if !selected.is_empty()
        && selected
            .iter()
            .any(|id| !rows.iter().any(|row| row.0 == *id))
    {
        return Err(Error::InvalidArguments);
    }
    let exact = match &receipt {
        ReceiptState::Intact(value) => effects::exact_effect_set(value, &rows)?,
        ReceiptState::PayloadErased(value) => opaque::exact_effect_set(value, &rows)?,
    };
    if !exact {
        return Err(Error::InternalInvariant);
    }
    let mut effects = Vec::new();
    for row in rows {
        effects.push(KnowledgeEffectReceipt {
            effect_id: row.0,
            kind: decode(serde_json::Value::String(row.1))?,
            status: decode(serde_json::Value::String(row.2))?,
            generation: row.3,
            owner_ref: row.4,
            detail: row.5,
        });
    }
    effects.sort_by_key(|effect| effects::rank(effect.kind));
    for effect in &mut effects {
        match &receipt {
            ReceiptState::Intact(value) => {
                effects::refresh(tx, tenant, workspace, effect, value, &completion).await?
            }
            ReceiptState::PayloadErased(value) => {
                opaque::refresh(tx, tenant, workspace, effect, value).await?
            }
        }
    }
    let required_complete = effects::required_complete(&completion, &effects);
    let remaining_work = effects
        .iter()
        .filter(|value| {
            value.status == KnowledgeEffectStatus::Pending
                || value.status == KnowledgeEffectStatus::Failed
        })
        .map(|value| format!("{:?}", value.kind).to_lowercase())
        .collect();
    let report = KnowledgeEffectsReport {
        publisher_receipt_id: receipt_id,
        effects,
        required_complete,
        remaining_work,
    };
    let next_phase = if required_complete {
        KnowledgeChangePhaseId::KcResultHandoff
    } else {
        KnowledgeChangePhaseId::KcSettleEffects
    };
    let updated = match &receipt {
        ReceiptState::Intact(_) => {
            sqlx::query("UPDATE knowledge_change_runs SET revision=revision+1,effects_report=$4,erased_effects_report=NULL,current_phase_id=$5,current_phase_ordinal=$6,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND NOT payload_erased")
                .bind(tenant).bind(workspace).bind(request.run_id).bind(json(&report)?).bind(Some(next_phase.as_str())).bind(next_phase.ordinal() as i32).execute(&mut **tx).await.map_err(storage_error)?.rows_affected()
        }
        ReceiptState::PayloadErased(_) => {
            let opaque = KnowledgeErasedEffectsReport::from_verified(&report);
            sqlx::query("UPDATE knowledge_change_runs SET revision=revision+1,effects_report=NULL,erased_effects_report=$4,current_phase_id=$5,current_phase_ordinal=$6,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND payload_erased")
                .bind(tenant).bind(workspace).bind(request.run_id).bind(json(&opaque)?).bind(Some(next_phase.as_str())).bind(next_phase.ordinal() as i32).execute(&mut **tx).await.map_err(storage_error)?.rows_affected()
        }
    };
    if updated != 1 {
        return Err(Error::ContextChanged);
    }
    let result = SettleKnowledgeChangeEffectsOutcome::Settled(report);
    if matches!(receipt, ReceiptState::PayloadErased(_)) {
        sqlx::query("INSERT INTO knowledge_lifecycle_command_receipts(tenant_id,workspace_id,operation,request_id,actor_principal_id,actor_session_id,request_payload,result_payload,payload_erased,erased_change_id) VALUES($1,$2,'settle_effects',$3,$4,$5,NULL,NULL,true,$6)")
            .bind(tenant).bind(workspace).bind(request.request_id).bind(principal).bind(session).bind(request.change_id).execute(&mut **tx).await.map_err(storage_error)?;
    } else {
        save_receipt(
            tx,
            tenant,
            workspace,
            principal,
            session,
            "settle_effects",
            request.request_id,
            &payload,
            &result,
        )
        .await?;
    }
    Ok(result)
}
