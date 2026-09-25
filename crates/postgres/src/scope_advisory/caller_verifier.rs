type CallerLinkRow = (
    Uuid,
    Uuid,
    Uuid,
    Uuid,
    Uuid,
    Uuid,
    String,
    Uuid,
    i64,
    Uuid,
    Uuid,
);
type VerifierReceiptRow = (Uuid, Uuid, Uuid, Uuid, Uuid, Uuid, Uuid, i64, String);

async fn persist_caller_link(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    input: &ScopeCallerLinkInput,
) -> Result<Uuid> {
    lock_scope_key(
        tx,
        tenant,
        workspace,
        "caller",
        input.preservation_receipt_id,
    )
    .await?;
    let (advice_header, _) = load_advice(
        tx,
        tenant,
        workspace,
        input.opportunity_id,
        Some(input.candidate_set_id),
    )
    .await?
    .ok_or(Error::NotFound)?;
    require_current_opportunity_config(
        tx,
        tenant,
        workspace,
        input.opportunity_id,
        input.candidate_set_id,
        advice_header.config_revision,
    )
    .await?;
    let lineage: Option<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT p.disposition_id,p.receipt_id FROM advisory_scope_preservation_receipt p \
         WHERE p.tenant_id=$1 AND p.workspace_id=$2 AND p.opportunity_id=$3 AND p.candidate_set_id=$4 \
           AND p.receipt_id=$5 AND p.status='passed'",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(input.opportunity_id)
    .bind(input.candidate_set_id)
    .bind(input.preservation_receipt_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if lineage != Some((input.disposition_id, input.preservation_receipt_id)) {
        return Err(Error::InputConflict);
    }
    let result_revision: Option<i64> = sqlx::query_scalar(
        "SELECT result_revision FROM scope_candidate_receipts WHERE tenant_id=$1 AND workspace_id=$2 \
         AND candidate_set_id=$3 AND operation=$4 AND request_id=$5",
    ).bind(tenant).bind(workspace).bind(input.candidate_set_id).bind(&input.caller_operation)
      .bind(input.caller_request_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    if result_revision != Some(input.caller_result_revision)
        || !actor_session_exists(tx, tenant, workspace, input.actor_id, input.session_id).await?
    {
        return Err(Error::InputConflict);
    }
    let existing: Option<CallerLinkRow> = sqlx::query_as(
        "SELECT link_id,request_id,opportunity_id,candidate_set_id,disposition_id,preservation_receipt_id,caller_operation,\
                caller_request_id,caller_result_revision,actor_id,session_id FROM advisory_scope_caller_link \
         WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3",
    ).bind(tenant).bind(workspace).bind(input.request_id)
      .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let expected = (
        input.link_id,
        input.request_id,
        input.opportunity_id,
        input.candidate_set_id,
        input.disposition_id,
        input.preservation_receipt_id,
        input.caller_operation.clone(),
        input.caller_request_id,
        input.caller_result_revision,
        input.actor_id,
        input.session_id,
    );
    if let Some(row) = existing {
        return if row == expected {
            Ok(row.0)
        } else {
            Err(Error::InputConflict)
        };
    }
    sqlx::query(
        "INSERT INTO advisory_scope_caller_link \
         (tenant_id,workspace_id,opportunity_id,candidate_set_id,link_id,request_id,disposition_id,\
          preservation_receipt_id,caller_operation,caller_request_id,caller_result_revision,actor_id,session_id) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    ).bind(tenant).bind(workspace).bind(input.opportunity_id).bind(input.candidate_set_id).bind(input.link_id)
      .bind(input.request_id).bind(input.disposition_id).bind(input.preservation_receipt_id)
      .bind(&input.caller_operation).bind(input.caller_request_id)
      .bind(input.caller_result_revision).bind(input.actor_id).bind(input.session_id)
      .execute(&mut **tx).await.map_err(storage_error)?;
    Ok(input.link_id)
}

async fn persist_verifier(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    input: &ScopeVerifierReceiptInput,
) -> Result<Uuid> {
    lock_scope_key(tx, tenant, workspace, "verifier", input.caller_link_id).await?;
    let identities: Option<(Uuid,Uuid,Uuid,Uuid,Uuid,i64)> = sqlx::query_as(
        "SELECT c.actor_id,c.session_id,d.actor_id,d.session_id,c.candidate_set_id,c.caller_result_revision \
         FROM advisory_scope_caller_link c JOIN advisory_scope_disposition d \
           ON (d.tenant_id,d.workspace_id,d.disposition_id)=(c.tenant_id,c.workspace_id,c.disposition_id) \
         WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.opportunity_id=$3 AND c.candidate_set_id=$4 \
           AND c.link_id=$5",
    ).bind(tenant).bind(workspace).bind(input.opportunity_id).bind(input.candidate_set_id)
      .bind(input.caller_link_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((caller_actor, caller_session, decision_actor, decision_session, candidate, revision)) =
        identities
    else {
        return Err(Error::NotFound);
    };
    if candidate != input.candidate_set_id
        || revision != input.verified_revision
        || !actor_session_exists(tx, tenant, workspace, input.actor_id, input.session_id).await?
        || (input.actor_id == caller_actor && input.session_id == caller_session)
        || (input.actor_id == decision_actor && input.session_id == decision_session)
    {
        return Err(Error::InputConflict);
    }
    let existing: Option<VerifierReceiptRow> = sqlx::query_as(
        "SELECT receipt_id,request_id,opportunity_id,candidate_set_id,caller_link_id,actor_id,session_id,verified_revision,verifier_digest \
         FROM advisory_scope_verifier_receipt WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3",
    ).bind(tenant).bind(workspace).bind(input.request_id)
      .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let expected = (
        input.receipt_id,
        input.request_id,
        input.opportunity_id,
        input.candidate_set_id,
        input.caller_link_id,
        input.actor_id,
        input.session_id,
        input.verified_revision,
        input.verifier_digest.clone(),
    );
    if let Some(row) = existing {
        return if row == expected {
            Ok(row.0)
        } else {
            Err(Error::InputConflict)
        };
    }
    sqlx::query(
        "INSERT INTO advisory_scope_verifier_receipt \
         (tenant_id,workspace_id,opportunity_id,candidate_set_id,receipt_id,request_id,caller_link_id,\
          actor_id,session_id,verified_revision,verifier_digest) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    ).bind(tenant).bind(workspace).bind(input.opportunity_id).bind(input.candidate_set_id)
      .bind(input.receipt_id).bind(input.request_id).bind(input.caller_link_id)
      .bind(input.actor_id).bind(input.session_id)
      .bind(input.verified_revision)
      .bind(&input.verifier_digest).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(input.receipt_id)
}
