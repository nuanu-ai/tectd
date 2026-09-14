use super::*;

struct PromotionSlice {
    id: Uuid,
    scope_id: Uuid,
    revision: i64,
}

async fn lock_promotion_slice(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    run: Uuid,
) -> Result<Option<PromotionSlice>> {
    let row: Option<(Uuid, Uuid, i64, String, String)> = sqlx::query_as(
        "SELECT id,scope_id,revision,state,pipeline FROM native_slices \
         WHERE tenant_id=$1 AND workspace_id=$2 AND knowledge_change_id=$3 \
         AND knowledge_run_id=$4 FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(change)
    .bind(run)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    row.map(|(id, scope_id, revision, state, pipeline)| {
        if state != "open" || pipeline != PipelineKind::PromoteToDurableKnowledge.as_str() {
            return Err(Error::StaleContext);
        }
        Ok(PromotionSlice {
            id,
            scope_id,
            revision,
        })
    })
    .transpose()
}

async fn publication(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    result: &KnowledgeChangeResult,
) -> Result<(Option<Uuid>, Option<String>, bool)> {
    match result.canonical {
        KnowledgeCanonicalOutcome::Applied => {
            let row: (Option<serde_json::Value>, Option<serde_json::Value>) = sqlx::query_as(
                "SELECT publisher_receipt,erased_publisher_receipt FROM knowledge_change_runs \
                 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
            )
            .bind(tenant)
            .bind(workspace)
            .bind(run)
            .fetch_one(&mut **tx)
            .await
            .map_err(storage_error)?;
            match row {
                (Some(value), None) => {
                    let receipt: KnowledgePublisherReceipt = decode(value)?;
                    if result.publisher_receipt_id != Some(receipt.id) {
                        return Err(Error::InputConflict);
                    }
                    Ok((Some(receipt.id), Some(receipt.digest), false))
                }
                (None, Some(value)) => {
                    let receipt: KnowledgeErasedPublisherReceipt = decode(value)?;
                    if result.publisher_receipt_id != Some(receipt.id) {
                        return Err(Error::InputConflict);
                    }
                    Ok((Some(receipt.id), None, true))
                }
                _ => Err(Error::InternalInvariant),
            }
        }
        KnowledgeCanonicalOutcome::NoChange if result.publisher_receipt_id.is_none() => {
            Ok((None, None, false))
        }
        KnowledgeCanonicalOutcome::Rejected if result.publisher_receipt_id.is_none() => {
            Ok((None, None, false))
        }
        _ => Err(Error::InvalidArguments),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn complete(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    change: Uuid,
    run: Uuid,
    attempt: Uuid,
    request: Uuid,
    definition_version: &str,
    definition_digest: &str,
    result: &KnowledgeChangeResult,
) -> Result<Option<Uuid>> {
    let Some(slice) = lock_promotion_slice(tx, tenant, workspace, change, run).await? else {
        return Ok(None);
    };
    let slice_outcome = match (result.canonical, result.user_outcome) {
        (
            KnowledgeCanonicalOutcome::Applied | KnowledgeCanonicalOutcome::NoChange,
            KnowledgeUserOutcome::Achieved,
        ) => "completed",
        (KnowledgeCanonicalOutcome::Rejected, KnowledgeUserOutcome::NotAchieved)
        | (
            KnowledgeCanonicalOutcome::Applied,
            KnowledgeUserOutcome::Partial | KnowledgeUserOutcome::NotAchieved,
        ) if !result.remaining_work.is_empty() => "blocked",
        _ => return Err(Error::InvalidArguments),
    };
    let (publisher_receipt_id, publisher_receipt_digest, receipt_erased) =
        publication(tx, tenant, workspace, run, result).await?;
    let result_revision: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(revision),0)+1 FROM slice_results \
         WHERE tenant_id=$1 AND workspace_id=$2 AND slice_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(slice.id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let id = Uuid::new_v4();
    let erased: bool = sqlx::query_scalar(
        "SELECT payload_erased FROM knowledge_change_runs \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let summary = if erased {
        "Durable Knowledge Change completed; semantic payload erased.".to_owned()
    } else {
        result.summary.clone()
    };
    let scope_impact = if erased {
        "Canonical erasure completion recorded by the Knowledge Change backend.".to_owned()
    } else {
        "Canonical durable knowledge outcome and required effects recorded.".to_owned()
    };
    let remaining_work = if erased || result.remaining_work.is_empty() {
        "none".to_owned()
    } else {
        result.remaining_work.join("; ")
    };
    let evidence = vec![SliceResultEvidence {
        kind: "knowledge_change_receipt".into(),
        reference: format!("urn:tect:knowledge-change:{change}:{run}"),
        observation: match result.canonical {
            KnowledgeCanonicalOutcome::Applied => "applied",
            KnowledgeCanonicalOutcome::NoChange => "no_change",
            KnowledgeCanonicalOutcome::Rejected => "rejected",
            _ => return Err(Error::InternalInvariant),
        }
        .into(),
    }];
    let origin = if receipt_erased {
        "applied_erased".to_owned()
    } else {
        enum_text(&result.canonical)?
    };
    let request_payload = serde_json::json!({
        "change_id": change,
        "run_id": run,
        "attempt_id": attempt,
        "canonical": result.canonical,
    });
    sqlx::query(
        "INSERT INTO slice_results \
         (id,tenant_id,workspace_id,scope_id,slice_id,slice_revision,revision,outcome,summary, \
          evidence,scope_impact,remaining_work,provenance,request_id,request_payload, \
          knowledge_change_id,knowledge_run_id,knowledge_definition_version, \
          knowledge_definition_digest,knowledge_final_attempt_id,knowledge_publisher_receipt_id, \
          knowledge_publisher_receipt_digest,knowledge_result_origin) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,'knowledge_change_managed', \
          $13,$14,$15,$16,$17,$18,$19,$20,$21,$22)",
    )
    .bind(id)
    .bind(tenant)
    .bind(workspace)
    .bind(slice.scope_id)
    .bind(slice.id)
    .bind(slice.revision)
    .bind(result_revision)
    .bind(slice_outcome)
    .bind(summary)
    .bind(json(&evidence)?)
    .bind(scope_impact)
    .bind(remaining_work)
    .bind(request)
    .bind(request_payload)
    .bind(change)
    .bind(run)
    .bind(definition_version)
    .bind(definition_digest)
    .bind(attempt)
    .bind(publisher_receipt_id)
    .bind(publisher_receipt_digest)
    .bind(origin)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    let next_slice_revision = slice
        .revision
        .checked_add(1)
        .ok_or(Error::StorageUnavailable)?;
    sqlx::query(
        "UPDATE native_slices SET revision=$5,state=$6 \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND knowledge_run_id=$4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(slice.id)
    .bind(run)
    .bind(next_slice_revision)
    .bind(slice_outcome)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    advance_planning(tx, tenant, workspace, session, &slice, id, result.canonical).await?;
    Ok(Some(id))
}

async fn advance_planning(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    slice: &PromotionSlice,
    result: Uuid,
    canonical: KnowledgeCanonicalOutcome,
) -> Result<()> {
    let set_id: Uuid = sqlx::query_scalar(
        "SELECT slice_candidate_set_id FROM native_scopes \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(slice.scope_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let (revision, latest_input): (i64, i64) = sqlx::query_as(
        "SELECT revision,latest_input FROM slice_candidate_sets \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(set_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let next_input = latest_input
        .checked_add(1)
        .ok_or(Error::StorageUnavailable)?;
    let input = format!(
        "Knowledge Change managed Slice Result {result}: {}",
        enum_text(&canonical)?
    );
    sqlx::query(
        "INSERT INTO slice_planning_inputs \
         (tenant_id,workspace_id,candidate_set_id,sequence,session_id,source_result_id,input) \
         VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(set_id)
    .bind(next_input)
    .bind(session)
    .bind(result)
    .bind(input)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    sqlx::query(
        "UPDATE slice_candidate_sets SET revision=$4,status='review_required',latest_input=$5 \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(set_id)
    .bind(revision.checked_add(1).ok_or(Error::StorageUnavailable)?)
    .bind(next_input)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    sqlx::query(
        "UPDATE native_scopes SET revision=revision+1 \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(slice.scope_id)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(())
}
