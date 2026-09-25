async fn consumed_budget(
    tx: &mut Transaction<'_, Postgres>, tenant: Uuid, workspace: Uuid, dispatch_id: Uuid,
) -> Result<Option<AdvisoryBudgetConsumption>> {
    let row: Option<(Uuid,Uuid,i64,String,Option<i64>,Option<i64>,Option<i64>,bool,bool)> =
        sqlx::query_as(
            "SELECT dispatch_id,policy_id,policy_version,policy_digest,input_tokens,\
             output_tokens,monotonic_elapsed_ms,unknown_usage,exhausted_after_response \
             FROM advisory_budget_consumptions WHERE tenant_id=$1 AND workspace_id=$2 AND dispatch_id=$3"
        ).bind(tenant).bind(workspace).bind(dispatch_id)
            .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    Ok(row.map(|r| AdvisoryBudgetConsumption {
        dispatch_id: r.0, policy_id: r.1, policy_version: r.2,
        policy_digest: r.3, input_tokens: r.4, output_tokens: r.5,
        monotonic_elapsed_ms: r.6, unknown_usage: r.7,
        exhausted_after_response: r.8,
    }))
}

fn consumption_exhausted(
    input_limit: i64, output_limit: i64, elapsed_limit: i64,
    previous_input: i64, previous_output: i64, previous_elapsed: i64,
    input: Option<i64>, output: Option<i64>, elapsed: Option<i64>,
    prior_exhausted: bool,
) -> Result<(bool,bool)> {
    let unknown = input.is_none() || output.is_none() || elapsed.is_none();
    let total_input = previous_input.checked_add(input.unwrap_or(0));
    let total_output = previous_output.checked_add(output.unwrap_or(0));
    let total_elapsed = previous_elapsed.checked_add(elapsed.unwrap_or(0));
    let over = total_input.is_none_or(|value| value > input_limit)
        || total_output.is_none_or(|value| value > output_limit)
        || total_elapsed.is_none_or(|value| value > elapsed_limit);
    Ok((unknown, unknown || over || prior_exhausted))
}

/// The caller must have committed the raw seal in a previous transaction.
/// This method is idempotent and locks the same dispatch/opportunity order as
/// start, so a concurrent replay cannot insert a second consumption.
async fn consume_budget(
    tx: &mut Transaction<'_, Postgres>, tenant: Uuid, workspace: Uuid, dispatch_id: Uuid,
) -> Result<AdvisoryBudgetConsumption> {
    let dispatch = dispatch_by_id(tx, tenant, workspace, dispatch_id, true).await?;
    if dispatch_state(&dispatch.state)? != AdvisoryDispatchState::Sealed {
        return Err(Error::InputConflict);
    }
    let _opportunity = opportunity_by_id(tx, tenant, workspace, dispatch.opportunity_id, true).await?;
    let reservation = reservation_for_dispatch(tx, tenant, workspace, dispatch_id)
        .await?.ok_or(Error::BudgetPolicyInvalid)?;
    if let Some(saved) = consumed_budget(tx, tenant, workspace, dispatch_id).await? {
        if saved.policy_id != reservation.policy_id
            || saved.policy_version != reservation.policy_version
            || saved.policy_digest != reservation.policy_digest
            || saved.input_tokens != dispatch.input_tokens
            || saved.output_tokens != dispatch.output_tokens
            || saved.monotonic_elapsed_ms != dispatch.latency_ms {
            return Err(Error::InputConflict);
        }
        return Ok(saved);
    }
    let ceilings: Option<(i64,i64,i64)> = sqlx::query_as(
        "SELECT input_tokens,output_tokens,elapsed_monotonic_ms FROM advisory_budget_policies \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND version=$4 AND digest=$5"
    ).bind(tenant).bind(workspace).bind(reservation.policy_id)
        .bind(reservation.policy_version).bind(&reservation.policy_digest)
        .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let (input_limit,output_limit,elapsed_limit) = ceilings.ok_or(Error::BudgetPolicyInvalid)?;
    let usage = crate::budget_policy_usage::policy_usage(
        tx,tenant,workspace,reservation.policy_id,reservation.policy_version,
        &reservation.policy_digest).await?;
    if usage.pending != 1 { return Err(Error::BudgetPolicyInvalid); }
    let (unknown, exhausted) = consumption_exhausted(
        input_limit,output_limit,elapsed_limit,
        usage.input_tokens,usage.output_tokens,usage.elapsed_ms,
        dispatch.input_tokens,dispatch.output_tokens,dispatch.latency_ms,
        usage.invalid != 0,
    )?;
    let inserted = sqlx::query(
        "INSERT INTO advisory_budget_consumptions \
         (tenant_id,workspace_id,opportunity_id,dispatch_id,policy_id,policy_version,\
          policy_digest,request_sha256,request_utf8_bytes,response_sha256,raw_response_ref,\
          send_certainty,outcome,input_tokens,output_tokens,input_tokens_known,\
          output_tokens_known,monotonic_elapsed_ms,elapsed_known,calls,retry_dispatches,\
          unknown_usage,exhausted_after_response,sealed_at) \
         SELECT d.tenant_id,d.workspace_id,d.opportunity_id,d.id,$4,$5,$6,$7,$8,\
          CASE WHEN d.response_payload IS NULL THEN NULL ELSE \
            pg_catalog.encode(pg_catalog.sha256(d.response_payload),'hex') END,\
          d.raw_response_ref,d.send_certainty,d.outcome,d.input_tokens,d.output_tokens,\
          d.input_tokens IS NOT NULL,d.output_tokens IS NOT NULL,d.latency_ms,\
          d.latency_ms IS NOT NULL,1,$9,$10,$11,d.sealed_at \
         FROM advisory_dispatch d WHERE d.tenant_id=$1 AND d.workspace_id=$2 \
           AND d.id=$3 AND d.state='sealed'"
    ).bind(tenant).bind(workspace).bind(dispatch_id).bind(reservation.policy_id)
        .bind(reservation.policy_version).bind(&reservation.policy_digest)
        .bind(&reservation.request_sha256).bind(reservation.request_utf8_bytes)
        .bind(reservation.reserved_retry_dispatches).bind(unknown).bind(exhausted)
        .execute(&mut **tx).await.map_err(storage_error)?;
    if inserted.rows_affected()!=1 { return Err(Error::InputConflict); }
    Ok(AdvisoryBudgetConsumption { dispatch_id, policy_id: reservation.policy_id,
        policy_version: reservation.policy_version, policy_digest: reservation.policy_digest,
        input_tokens: dispatch.input_tokens, output_tokens: dispatch.output_tokens,
        monotonic_elapsed_ms: dispatch.latency_ms, unknown_usage: unknown,
        exhausted_after_response: exhausted })
}

#[cfg(test)]
mod budget_consumption_tests {
    use super::*;
    #[test]
    fn exact_limits_overrun_and_unknown_are_distinct() {
        assert_eq!(consumption_exhausted(10,20,100,0,0,0,Some(10),Some(20),Some(100),false),Ok((false,false)));
        assert_eq!(consumption_exhausted(10,20,100,0,0,0,Some(11),Some(20),Some(100),false),Ok((false,true)));
        assert_eq!(consumption_exhausted(10,20,100,0,0,0,Some(10),Some(21),Some(100),false),Ok((false,true)));
        assert_eq!(consumption_exhausted(10,20,100,0,0,0,Some(10),Some(20),Some(101),false),Ok((false,true)));
        assert_eq!(consumption_exhausted(10,20,100,0,0,0,None,Some(1),Some(1),false),Ok((true,true)));
        assert_eq!(consumption_exhausted(10,20,100,0,0,0,Some(1),None,Some(1),false),Ok((true,true)));
        assert_eq!(consumption_exhausted(10,20,100,0,0,0,Some(1),Some(1),None,false),Ok((true,true)));
    }
}
