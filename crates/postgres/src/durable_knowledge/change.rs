use super::*;

fn semantic_diff(
    operation: KnowledgeOperation,
    baseline: Option<&KnowledgeUnitRevision>,
    draft: Option<&KnowledgeConstraintDraft>,
) -> String {
    match operation {
        KnowledgeOperation::Create=>"Creates one accepted General-DK execution constraint with the supplied source snapshot and binding.".into(),
        KnowledgeOperation::Retract=>format!("Retracts accepted revision {} from current delivery while preserving its content and history.",baseline.map(|v|v.revision).unwrap_or_default()),
        KnowledgeOperation::Revise=>{
            let Some(old)=baseline.map(|v|&v.constraint) else{return "Revises the accepted constraint.".into()}; let Some(new)=draft else{return "Revises the accepted constraint.".into()};
            let mut changed=Vec::new();
            if old.title!=new.title{changed.push("title")}; if old.statement!=new.statement{changed.push("statement")}; if old.modality!=new.modality{changed.push("modality")}; if old.action!=new.action{changed.push("action")}; if old.target_iri!=new.target_iri{changed.push("target")}; if old.conditions!=new.conditions{changed.push("conditions")}; if old.exceptions!=new.exceptions{changed.push("exceptions")}; if old.source!=new.source{changed.push("source snapshot")}; if old.binding!=new.binding{changed.push("execution binding")};
            format!("Revises accepted revision {}. Changed: {}.",baseline.map(|v|v.revision).unwrap_or_default(),if changed.is_empty(){"no material proposal fields".into()}else{changed.join(", ")})
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn prepare(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Uuid,
    request: &PrepareKnowledgeChange,
    binding: Option<&KnowledgeBindingProvenance>,
    preparation: &KnowledgeMethodSnapshot,
    review: &KnowledgeMethodSnapshot,
) -> Result<PrepareKnowledgeChangeOutcome> {
    let payload = json(request)?;
    if let Some(prior) = receipt::<PrepareKnowledgeChangeOutcome>(
        tx,
        tenant,
        workspace,
        "prepare",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(match prior {
            PrepareKnowledgeChangeOutcome::Prepared(v)
            | PrepareKnowledgeChangeOutcome::Replay(v) => PrepareKnowledgeChangeOutcome::Replay(v),
            other => other,
        });
    }
    let (generation, ready, _) = lock_state(tx, tenant, workspace).await?;
    if let Some(prior) = receipt::<PrepareKnowledgeChangeOutcome>(
        tx,
        tenant,
        workspace,
        "prepare",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(match prior {
            PrepareKnowledgeChangeOutcome::Prepared(v)
            | PrepareKnowledgeChangeOutcome::Replay(v) => PrepareKnowledgeChangeOutcome::Replay(v),
            other => other,
        });
    }
    if generation != request.expected_generation {
        return Err(Error::StaleContext);
    }
    let unit = request.unit_id.unwrap_or_else(Uuid::new_v4);
    let baseline = match request.operation {
        KnowledgeOperation::Create => None,
        _ => {
            if !ready {
                return Err(Error::KnowledgeUnavailable);
            }
            let value = context::load_revision(tx, tenant, workspace, unit, None, true)
                .await?
                .ok_or(Error::NotFound)?;
            if value.revision != request.expected_unit_revision.unwrap_or_default() {
                return Err(Error::StaleRevision);
            }
            if request.operation == KnowledgeOperation::Retract && !value.active {
                return Err(Error::StaleRevision);
            }
            Some(value)
        }
    };
    let fingerprint = request.draft.as_ref().map(fingerprint).transpose()?;
    if request.operation==KnowledgeOperation::Create
        && let Some(existing)=sqlx::query_scalar::<_,Uuid>("SELECT unit_id FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 AND active AND proposal_fingerprint=$3 LIMIT 1")
            .bind(tenant).bind(workspace).bind(fingerprint.as_deref()).fetch_optional(&mut **tx).await.map_err(storage_error)? {
        let outcome=PrepareKnowledgeChangeOutcome::Duplicate{existing_unit_id:existing}; save_receipt(tx,tenant,workspace,"prepare",request.request_id,session,&payload,&outcome).await?; return Ok(outcome)
    }
    let proposed = match request.operation {
        KnowledgeOperation::Create => 1,
        KnowledgeOperation::Revise => baseline.as_ref().unwrap().revision + 1,
        KnowledgeOperation::Retract => baseline.as_ref().unwrap().revision,
    };
    let source_sha = request
        .draft
        .as_ref()
        .map(|draft| sha256(draft.source.text.as_bytes()));
    let proposal_digest = digest(&json(&(
        &request.operation,
        unit,
        request.expected_generation,
        request.expected_unit_revision,
        proposed,
        &request.draft,
        &request.reason,
        &request.authority_basis,
        binding,
        preparation,
        review,
    ))?)?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO knowledge_changes(id,tenant_id,workspace_id,unit_id,operation,stage,expected_generation,expected_unit_revision,proposed_unit_revision,proposal_digest,proposal_fingerprint,source_sha256,semantic_diff,baseline,proposal,binding_provenance,preparation_method,review_method,reason,authority_basis,prepared_principal_id,prepared_session_id) VALUES($1,$2,$3,$4,$5,'review_required',$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21)")
        .bind(id).bind(tenant).bind(workspace).bind(unit).bind(operation(request.operation)).bind(request.expected_generation).bind(request.expected_unit_revision).bind(proposed).bind(&proposal_digest).bind(&fingerprint).bind(&source_sha).bind(semantic_diff(request.operation,baseline.as_ref(),request.draft.as_ref())).bind(baseline.as_ref().map(json).transpose()?).bind(request.draft.as_ref().map(json).transpose()?).bind(binding.map(json).transpose()?).bind(json(preparation)?).bind(json(review)?).bind(&request.reason).bind(&request.authority_basis).bind(principal).bind(session)
        .execute(&mut **tx).await.map_err(storage_error)?;
    let value = context::load_change(tx, tenant, workspace, id)
        .await?
        .ok_or(Error::InternalInvariant)?;
    let outcome = PrepareKnowledgeChangeOutcome::Prepared(value);
    save_receipt(
        tx,
        tenant,
        workspace,
        "prepare",
        request.request_id,
        session,
        &payload,
        &outcome,
    )
    .await?;
    Ok(outcome)
}

#[allow(clippy::type_complexity)]
pub(crate) async fn review(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Uuid,
    request: &ReviewKnowledgeChange,
) -> Result<ReviewKnowledgeChangeOutcome> {
    let payload = json(request)?;
    if let Some(prior) = receipt::<ReviewKnowledgeChangeOutcome>(
        tx,
        tenant,
        workspace,
        "review",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(match prior {
            ReviewKnowledgeChangeOutcome::Approved(v)
            | ReviewKnowledgeChangeOutcome::Rejected(v)
            | ReviewKnowledgeChangeOutcome::Replay(v) => ReviewKnowledgeChangeOutcome::Replay(v),
        });
    }
    let (generation, _, _) = lock_state(tx, tenant, workspace).await?;
    if let Some(prior) = receipt::<ReviewKnowledgeChangeOutcome>(
        tx,
        tenant,
        workspace,
        "review",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(match prior {
            ReviewKnowledgeChangeOutcome::Approved(v)
            | ReviewKnowledgeChangeOutcome::Rejected(v)
            | ReviewKnowledgeChangeOutcome::Replay(v) => ReviewKnowledgeChangeOutcome::Replay(v),
        });
    }
    let row:Option<(i64,String,String,i64,Option<i64>,Uuid,serde_json::Value)>=sqlx::query_as("SELECT change_revision,stage,proposal_digest,expected_generation,expected_unit_revision,unit_id,review_method FROM knowledge_changes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(request.change_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let (
        revision,
        current_stage,
        proposal_digest,
        expected_generation,
        expected_unit,
        unit,
        method,
    ) = row.ok_or(Error::NotFound)?;
    if revision != request.change_revision
        || current_stage != "review_required"
        || proposal_digest != request.proposal_digest
    {
        return Err(Error::StaleRevision);
    }
    let expected_method: KnowledgeMethodSnapshot = decode(method)?;
    if request.method_read.id != expected_method.id
        || request.method_read.version != expected_method.version
        || request.method_read.digest != expected_method.digest
    {
        return Err(Error::StaleContext);
    }
    if generation != expected_generation {
        return Err(Error::StaleContext);
    }
    if let Some(expected) = expected_unit {
        let head:Option<i64>=sqlx::query_scalar("SELECT accepted_revision FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3").bind(tenant).bind(workspace).bind(unit).fetch_optional(&mut **tx).await.map_err(storage_error)?;
        if head != Some(expected) {
            return Err(Error::StaleRevision);
        }
    }
    let review = KnowledgeReview {
        verdict: request.verdict,
        summary: request.review_summary.clone(),
        method_read: request.method_read.clone(),
        reviewer_principal_id: principal,
        reviewer_session_id: session,
    };
    let next = match request.verdict {
        KnowledgeReviewVerdict::Approve => "ready_to_publish",
        KnowledgeReviewVerdict::Reject => "rejected",
    };
    sqlx::query("UPDATE knowledge_changes SET stage=$4,review=$5,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(request.change_id).bind(next).bind(json(&review)?).execute(&mut **tx).await.map_err(storage_error)?;
    let value = context::load_change(tx, tenant, workspace, request.change_id)
        .await?
        .ok_or(Error::InternalInvariant)?;
    let outcome = match request.verdict {
        KnowledgeReviewVerdict::Approve => ReviewKnowledgeChangeOutcome::Approved(value),
        KnowledgeReviewVerdict::Reject => ReviewKnowledgeChangeOutcome::Rejected(value),
    };
    save_receipt(
        tx,
        tenant,
        workspace,
        "review",
        request.request_id,
        session,
        &payload,
        &outcome,
    )
    .await?;
    Ok(outcome)
}
