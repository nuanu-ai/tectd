fn remaining_budget_envelope(
    policy: &AdvisoryBudgetPolicy,
    prior_calls: i64,
    prior_bytes: i64,
    prior_retries: i64,
    request_bytes: i64,
    is_retry: bool,
    monotonic_elapsed_ms: Option<i64>,
) -> Result<i64> {
    let c = policy.ceilings();
    if prior_calls < 0 || prior_bytes < 0 || prior_retries < 0 || request_bytes <= 0 {
        return Err(Error::InternalInvariant);
    }
    let consumed_elapsed = if is_retry {
        // A retry without a trusted cumulative monotonic reading is denied.
        monotonic_elapsed_ms.ok_or(Error::BudgetPolicyInvalid)?
    } else {
        0
    };
    if consumed_elapsed < 0 {
        return Err(Error::BudgetPolicyInvalid);
    }
    let calls = prior_calls.checked_add(1).ok_or(Error::BudgetExhaustedBeforeDispatch)?;
    let bytes = prior_bytes.checked_add(request_bytes).ok_or(Error::BudgetExhaustedBeforeDispatch)?;
    let retries = prior_retries.checked_add(i64::from(is_retry))
        .ok_or(Error::BudgetExhaustedBeforeDispatch)?;
    if calls > c.provider_calls || bytes > c.request_utf8_bytes
        || retries > c.retry_dispatches || consumed_elapsed >= c.elapsed_monotonic_ms {
        return Err(Error::BudgetExhaustedBeforeDispatch);
    }
    Ok(c.elapsed_monotonic_ms - consumed_elapsed)
}

async fn reservation_for_dispatch(
    tx: &mut Transaction<'_, Postgres>, tenant: Uuid, workspace: Uuid, dispatch_id: Uuid,
) -> Result<Option<AdvisoryBudgetReservation>> {
    let row: Option<(Uuid,Uuid,i64,String,i64,i64,String,i64,i64,i64,i64,i64,i64)> = sqlx::query_as(
        "SELECT dispatch_id,policy_id,policy_version,policy_digest,\
         policy_effective_from_unix_ms,policy_effective_until_unix_ms,request_sha256,\
         request_utf8_bytes,reserved_calls,reserved_retry_dispatches,remaining_elapsed_ms,\
         reserved_input_tokens,reserved_output_tokens \
         FROM advisory_budget_reservations WHERE tenant_id=$1 AND workspace_id=$2 AND dispatch_id=$3"
    ).bind(tenant).bind(workspace).bind(dispatch_id)
        .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    Ok(row.map(|r| AdvisoryBudgetReservation {
        dispatch_id: r.0, policy_id: r.1, policy_version: r.2,
        policy_digest: r.3, policy_effective_from_unix_ms: r.4,
        policy_effective_until_unix_ms: r.5, request_sha256: r.6,
        request_utf8_bytes: r.7, reserved_calls: r.8,
        reserved_retry_dispatches: r.9, remaining_elapsed_ms: r.10,
        reserved_input_tokens: r.11, reserved_output_tokens: r.12,
    }))
}

/// Called only after the application supplied an authenticated policy. The
/// opportunity row is already locked by start_dispatch, serializing siblings.
async fn reserve_before_dispatch(
    tx: &mut Transaction<'_, Postgres>, tenant: Uuid, workspace: Uuid,
    row: &DispatchRow, policy: Option<&AdvisoryBudgetPolicy>,
    monotonic_elapsed_ms: Option<i64>,
) -> Result<AdvisoryBudgetReservation> {
    let policy = policy.ok_or(Error::BudgetPolicyInvalid)?;
    policy.validate().map_err(|_| Error::BudgetPolicyInvalid)?;
    if reservation_for_dispatch(tx, tenant, workspace, row.id).await?.is_some() {
        return Err(Error::InputConflict);
    }
    let now: i64 = sqlx::query_scalar(
        "SELECT (EXTRACT(EPOCH FROM pg_catalog.clock_timestamp())*1000)::bigint"
    ).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if !policy.is_effective_at(now) {
        return Err(Error::BudgetPolicyInvalid);
    }
    let installed: Option<(Uuid,i64,String,i64,i64)> = sqlx::query_as(
        "SELECT id,version,digest,effective_from_unix_ms,effective_until_unix_ms \
         FROM advisory_budget_policies WHERE tenant_id=$1 AND workspace_id=$2 \
           AND effective_from_unix_ms<=$3 AND effective_until_unix_ms>$3 \
         ORDER BY version DESC LIMIT 1"
    ).bind(tenant).bind(workspace).bind(now)
        .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    if installed.as_ref().is_none_or(|current|
        current.0 != policy.id() || current.1 != policy.version()
        || current.2 != policy.digest()
        || current.3 != policy.effective_from_unix_ms()
        || current.4 != policy.effective_until_unix_ms()) {
        return Err(Error::BudgetPolicyInvalid);
    }
    let (prior_calls,prior_bytes,prior_retries,foreign_policy): (i64,i64,i64,i64) =
        sqlx::query_as(
            "SELECT COUNT(*)::bigint,COALESCE(SUM(request_utf8_bytes),0)::bigint,\
             COALESCE(SUM(reserved_retry_dispatches),0)::bigint,\
             COUNT(*) FILTER (WHERE policy_id<>$4 OR policy_version<>$5 OR policy_digest<>$6)::bigint \
             FROM advisory_budget_reservations WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3"
        ).bind(tenant).bind(workspace).bind(row.opportunity_id)
            .bind(policy.id()).bind(policy.version()).bind(policy.digest())
            .fetch_one(&mut **tx).await.map_err(storage_error)?;
    if foreign_policy != 0 { return Err(Error::BudgetPolicyInvalid); }
    let (pending,used_input,used_output,invalid_consumption): (i64,i64,i64,i64) =
        sqlx::query_as(
            "SELECT COUNT(*) FILTER (WHERE c.dispatch_id IS NULL)::bigint,\
             COALESCE(SUM(c.input_tokens),0)::bigint,\
             COALESCE(SUM(c.output_tokens),0)::bigint,\
             COUNT(*) FILTER (WHERE c.unknown_usage OR c.exhausted_after_response)::bigint \
             FROM advisory_budget_reservations r LEFT JOIN advisory_budget_consumptions c \
             ON (c.tenant_id,c.workspace_id,c.dispatch_id)=(r.tenant_id,r.workspace_id,r.dispatch_id) \
             WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.opportunity_id=$3"
        ).bind(tenant).bind(workspace).bind(row.opportunity_id)
            .fetch_one(&mut **tx).await.map_err(storage_error)?;
    if pending != 0 || invalid_consumption != 0 { return Err(Error::BudgetPolicyInvalid); }
    let remaining_input = policy.ceilings().input_tokens.checked_sub(used_input)
        .ok_or(Error::BudgetExhaustedBeforeDispatch)?;
    let remaining_output = policy.ceilings().output_tokens.checked_sub(used_output)
        .ok_or(Error::BudgetExhaustedBeforeDispatch)?;
    if remaining_input <= 0 || remaining_output <= 0 {
        return Err(Error::BudgetExhaustedBeforeDispatch);
    }
    let request_bytes = i64::try_from(row.request_payload.len())
        .map_err(|_| Error::BudgetExhaustedBeforeDispatch)?;
    let is_retry = row.attempt_number > 1;
    let remaining = remaining_budget_envelope(policy, prior_calls, prior_bytes,
        prior_retries, request_bytes, is_retry, monotonic_elapsed_ms)?;
    let reservation = AdvisoryBudgetReservation {
        dispatch_id: row.id, policy_id: policy.id(), policy_version: policy.version(),
        policy_digest: policy.digest().to_owned(),
        policy_effective_from_unix_ms: policy.effective_from_unix_ms(),
        policy_effective_until_unix_ms: policy.effective_until_unix_ms(),
        request_sha256: row.payload_digest.clone(), request_utf8_bytes: request_bytes,
        reserved_calls: 1, reserved_retry_dispatches: i64::from(is_retry),
        remaining_elapsed_ms: remaining,
        reserved_input_tokens: remaining_input, reserved_output_tokens: remaining_output,
    };
    sqlx::query(
        "INSERT INTO advisory_budget_reservations \
         (tenant_id,workspace_id,opportunity_id,dispatch_id,policy_id,policy_version,\
         policy_digest,policy_effective_from_unix_ms,policy_effective_until_unix_ms,\
         request_sha256,request_utf8_bytes,reserved_retry_dispatches,remaining_elapsed_ms,\
         reserved_input_tokens,reserved_output_tokens) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)"
    ).bind(tenant).bind(workspace).bind(row.opportunity_id).bind(row.id)
        .bind(reservation.policy_id).bind(reservation.policy_version)
        .bind(&reservation.policy_digest).bind(reservation.policy_effective_from_unix_ms)
        .bind(reservation.policy_effective_until_unix_ms).bind(&reservation.request_sha256)
        .bind(reservation.request_utf8_bytes).bind(reservation.reserved_retry_dispatches)
        .bind(reservation.remaining_elapsed_ms)
        .bind(reservation.reserved_input_tokens).bind(reservation.reserved_output_tokens)
        .execute(&mut **tx).await.map_err(storage_error)?;
    Ok(reservation)
}

#[cfg(test)]
mod budget_reservation_tests {
    use super::*;
    fn policy() -> AdvisoryBudgetPolicy {
        let id=Uuid::new_v4();
        let c=AdvisoryBudgetCeilings { provider_calls: 2, input_tokens: 1,
            output_tokens: 1, request_utf8_bytes: 10, elapsed_monotonic_ms: 100,
            retry_dispatches: 1 };
        AdvisoryBudgetPolicy::new(id,1,AdvisoryBudgetPolicy::digest_for(id,1,0,200,c),
            0,200,c,Uuid::new_v4(),"a".repeat(128)).unwrap()
    }
    #[test]
    fn exact_edges_retry_and_unknown_monotonic_elapsed() {
        let p=policy();
        assert_eq!(remaining_budget_envelope(&p,0,0,0,10,false,None),Ok(100));
        assert_eq!(remaining_budget_envelope(&p,0,0,0,11,false,None),
            Err(Error::BudgetExhaustedBeforeDispatch));
        assert_eq!(remaining_budget_envelope(&p,1,5,0,5,true,Some(99)),Ok(1));
        assert_eq!(remaining_budget_envelope(&p,1,5,0,5,true,Some(100)),
            Err(Error::BudgetExhaustedBeforeDispatch));
        assert_eq!(remaining_budget_envelope(&p,1,5,0,5,true,None),
            Err(Error::BudgetPolicyInvalid));
        assert_eq!(remaining_budget_envelope(&p,2,0,0,1,false,None),
            Err(Error::BudgetExhaustedBeforeDispatch));
        assert_eq!(remaining_budget_envelope(&p,0,0,1,1,true,Some(1)),
            Err(Error::BudgetExhaustedBeforeDispatch));
    }
}
