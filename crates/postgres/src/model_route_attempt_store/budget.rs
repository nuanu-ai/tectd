//! Reservation and immutable consumption of a model route's shared policy budget.
use super::*;

pub(super) async fn begin_send(
    uow: &mut PgUnitOfWork,
    prepared: &PreparedModelRouteRecommendation,
    invocation: ModelRouteInvocation,
    attempted: &ModelRoutePreparedAttempt,
    policy: &AdvisoryBudgetPolicy,
) -> Result<Option<ModelRouteSendPermit>> {
    stored_preparation(uow, prepared).await?;
    if prepared.preparation != tect_application::ModelRoutePreparation::Prepared {
        return Err(Error::InputConflict);
    }
    attempted.verify(prepared)?;
    let principal = authenticated_invocation(uow, prepared.workspace_id, invocation).await?;
    if let Some(row) = attempt_row(uow, prepared.workspace_id, &prepared.request_key).await? {
        let same = row
            .try_get::<Option<Vec<u8>>, _>("request_payload")
            .map_err(storage_error)?
            == Some(attempted.request_bytes.clone())
            && row
                .try_get::<Option<String>, _>("request_sha256")
                .map_err(storage_error)?
                == Some(attempted.request_sha256.clone())
            && row
                .try_get::<Uuid, _>("invoking_session_id")
                .map_err(storage_error)?
                == invocation.session_id
            && row
                .try_get::<Uuid, _>("invoking_principal_id")
                .map_err(storage_error)?
                == principal;
        return if same {
            Ok(None)
        } else {
            Err(Error::InputConflict)
        };
    }
    policy.validate().map_err(|_| Error::BudgetPolicyInvalid)?;
    let tenant = uow.tenant_id()?;
    crate::budget_policy_usage::lock_workspace_policy(
        uow.transaction()?,
        tenant,
        prepared.workspace_id,
    )
    .await?;
    let now: i64 = sqlx::query_scalar(
        "SELECT (EXTRACT(EPOCH FROM pg_catalog.clock_timestamp())*1000)::bigint",
    )
    .fetch_one(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    if !policy.is_effective_at(now) {
        return Err(Error::BudgetPolicyInvalid);
    }
    let request_len = i64::try_from(attempted.request_bytes.len())
        .map_err(|_| Error::BudgetExhaustedBeforeDispatch)?;
    let ceilings = policy.ceilings();
    if request_len == 0 || request_len > ceilings.request_utf8_bytes {
        return Err(Error::BudgetExhaustedBeforeDispatch);
    }
    let usage = crate::budget_policy_usage::policy_usage(
        uow.transaction()?,
        tenant,
        prepared.workspace_id,
        policy.id(),
        policy.version(),
        policy.digest(),
    )
    .await?;
    crate::budget_policy_usage::require_current_policy(
        uow.transaction()?,
        tenant,
        prepared.workspace_id,
        policy,
    )
    .await?;
    if usage.pending != 0 || usage.invalid != 0 {
        return Err(Error::BudgetPolicyInvalid);
    }
    if usage
        .calls
        .checked_add(1)
        .is_none_or(|n| n > ceilings.provider_calls)
        || usage
            .request_bytes
            .checked_add(request_len)
            .is_none_or(|n| n > ceilings.request_utf8_bytes)
    {
        return Err(Error::BudgetExhaustedBeforeDispatch);
    }
    let input = ceilings
        .input_tokens
        .checked_sub(usage.input_tokens)
        .ok_or(Error::BudgetExhaustedBeforeDispatch)?;
    let output = ceilings
        .output_tokens
        .checked_sub(usage.output_tokens)
        .ok_or(Error::BudgetExhaustedBeforeDispatch)?;
    let elapsed = ceilings
        .elapsed_monotonic_ms
        .checked_sub(usage.elapsed_ms)
        .ok_or(Error::BudgetExhaustedBeforeDispatch)?;
    if input <= 0 || output <= 0 || elapsed <= 0 {
        return Err(Error::BudgetExhaustedBeforeDispatch);
    }
    let id = insert_attempt(
        uow,
        prepared,
        invocation,
        principal,
        "send_unknown",
        None,
        Some(attempted),
    )
    .await?;
    sqlx::query(
        "INSERT INTO model_route_budget_reservations (tenant_id,workspace_id,attempt_id, \
         policy_id,policy_version,policy_digest,request_sha256,request_utf8_bytes, \
         reserved_input_tokens,reserved_output_tokens,reserved_elapsed_ms) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(tenant)
    .bind(prepared.workspace_id)
    .bind(id)
    .bind(policy.id())
    .bind(policy.version())
    .bind(policy.digest())
    .bind(&attempted.request_sha256)
    .bind(request_len)
    .bind(input)
    .bind(output)
    .bind(elapsed)
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(write_error)?;
    Ok(Some(ModelRouteSendPermit {
        attempt_id: id,
        workspace_id: prepared.workspace_id,
        preparation_request_key: prepared.request_key.clone(),
        request_sha256: attempted.request_sha256.clone(),
        policy_id: policy.id(),
        policy_version: policy.version(),
        policy_digest: policy.digest().to_owned(),
    }))
}

type Consumption = (Option<i64>, Option<i64>, Option<i64>, bool);

async fn existing_consumption(
    uow: &mut PgUnitOfWork,
    tenant: Uuid,
    permit: &ModelRouteSendPermit,
) -> Result<Option<Consumption>> {
    sqlx::query_as(
        "SELECT input_tokens,output_tokens,elapsed_monotonic_ms,exhausted_after_response \
         FROM model_route_budget_consumptions WHERE tenant_id=$1 AND workspace_id=$2 AND attempt_id=$3",
    )
    .bind(tenant)
    .bind(permit.workspace_id)
    .bind(permit.attempt_id)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)
}

fn replay_outcome(
    existing: Consumption,
    observation: &ModelRouteProviderObservation,
) -> Result<bool> {
    if existing.0 != observation.input_tokens
        || existing.1 != observation.output_tokens
        || existing.2 != observation.elapsed_monotonic_ms
    {
        return Err(Error::InputConflict);
    }
    Ok(existing.3)
}

pub(super) async fn consume_budget(
    uow: &mut PgUnitOfWork,
    permit: &ModelRouteSendPermit,
    observation: &ModelRouteProviderObservation,
) -> Result<bool> {
    if !uow.is_read_write() {
        return Err(Error::Forbidden);
    }
    let row = permit_row(uow, permit).await?;
    let state: String = row.try_get("state").map_err(storage_error)?;
    if state != "raw_sealed" && state != "parsed" {
        return Err(Error::InputConflict);
    }
    let original = reads::observation_from_row(&row)?.ok_or(Error::StaleContext)?;
    if original.http_status != observation.http_status
        || original.elapsed_monotonic_ms != observation.elapsed_monotonic_ms
    {
        return Err(Error::InputConflict);
    }
    let response_digest = model_route_wire_sha256(&observation.raw);
    if row
        .try_get::<Option<Vec<u8>>, _>("response_payload")
        .map_err(storage_error)?
        != Some(observation.raw.clone())
        || row
            .try_get::<Option<String>, _>("response_sha256")
            .map_err(storage_error)?
            != Some(response_digest.clone())
    {
        return Err(Error::InputConflict);
    }
    let tenant = uow.tenant_id()?;
    if let Some(existing) = existing_consumption(uow, tenant, permit).await? {
        return replay_outcome(existing, observation);
    }
    let limits: Option<(i64, i64, i64)> = sqlx::query_as(
        "SELECT reserved_input_tokens,reserved_output_tokens,reserved_elapsed_ms \
         FROM model_route_budget_reservations WHERE tenant_id=$1 AND workspace_id=$2 AND attempt_id=$3",
    )
    .bind(tenant)
    .bind(permit.workspace_id)
    .bind(permit.attempt_id)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    let (input_limit, output_limit, elapsed_limit) = limits.ok_or(Error::InputConflict)?;
    let usage = crate::budget_policy_usage::policy_usage(
        uow.transaction()?,
        tenant,
        permit.workspace_id,
        permit.policy_id,
        permit.policy_version,
        &permit.policy_digest,
    )
    .await?;
    // The policy lock may have waited for another replay to commit. A fresh
    // READ COMMITTED query sees that immutable row and returns its saved result.
    if let Some(existing) = existing_consumption(uow, tenant, permit).await? {
        return replay_outcome(existing, observation);
    }
    if usage.pending != 1 {
        return Err(Error::BudgetPolicyInvalid);
    }
    let unknown = observation.input_tokens.is_none()
        || observation.output_tokens.is_none()
        || observation.elapsed_monotonic_ms.is_none();
    let exhausted = unknown
        || usage.invalid != 0
        || observation
            .input_tokens
            .is_some_and(|n| n < 0 || n > input_limit)
        || observation
            .output_tokens
            .is_some_and(|n| n < 0 || n > output_limit)
        || observation
            .elapsed_monotonic_ms
            .is_some_and(|n| n < 0 || n > elapsed_limit);
    sqlx::query(
        "INSERT INTO model_route_budget_consumptions (tenant_id,workspace_id,attempt_id, \
         policy_id,policy_version,policy_digest,request_sha256,response_sha256, \
         input_tokens,output_tokens,elapsed_monotonic_ms,unknown_usage, \
         exhausted_after_response,transport_failed) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,false)",
    )
    .bind(tenant)
    .bind(permit.workspace_id)
    .bind(permit.attempt_id)
    .bind(permit.policy_id)
    .bind(permit.policy_version)
    .bind(&permit.policy_digest)
    .bind(&permit.request_sha256)
    .bind(response_digest)
    .bind(observation.input_tokens)
    .bind(observation.output_tokens)
    .bind(observation.elapsed_monotonic_ms)
    .bind(unknown)
    .bind(exhausted)
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(write_error)?;
    Ok(exhausted)
}

pub(super) async fn mark_send_unknown(
    uow: &mut PgUnitOfWork,
    permit: &ModelRouteSendPermit,
) -> Result<()> {
    let row = permit_row(uow, permit).await?;
    let state: String = row.try_get("state").map_err(storage_error)?;
    if state != "send_unknown" {
        return Err(Error::InputConflict);
    }
    let tenant = uow.tenant_id()?;
    sqlx::query(
        "INSERT INTO model_route_budget_consumptions (tenant_id,workspace_id,attempt_id, \
         policy_id,policy_version,policy_digest,request_sha256,response_sha256, \
         input_tokens,output_tokens,elapsed_monotonic_ms,unknown_usage, \
         exhausted_after_response,transport_failed) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,NULL,NULL,NULL,NULL,true,true,true) \
         ON CONFLICT (tenant_id,workspace_id,attempt_id) DO NOTHING",
    )
    .bind(tenant)
    .bind(permit.workspace_id)
    .bind(permit.attempt_id)
    .bind(permit.policy_id)
    .bind(permit.policy_version)
    .bind(&permit.policy_digest)
    .bind(&permit.request_sha256)
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(write_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immutable_replay_result_is_reused_and_mismatch_is_rejected() {
        let observation = ModelRouteProviderObservation {
            raw: vec![],
            http_status: None,
            input_tokens: Some(3),
            output_tokens: Some(5),
            elapsed_monotonic_ms: Some(7),
        };
        assert_eq!(
            replay_outcome((Some(3), Some(5), Some(7), true), &observation),
            Ok(true)
        );
        assert_eq!(
            replay_outcome((Some(3), Some(5), Some(7), false), &observation),
            Ok(false)
        );
        assert!(matches!(
            replay_outcome((Some(4), Some(5), Some(7), true), &observation),
            Err(Error::InputConflict)
        ));
    }
}
