use super::*;

pub(super) async fn persist_verifier(
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
    type Existing = (Uuid, Uuid, Uuid, Uuid, Uuid, Uuid, Uuid, i64, String);
    let existing: Option<Existing> = sqlx::query_as(
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
