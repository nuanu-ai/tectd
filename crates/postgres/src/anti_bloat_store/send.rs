use super::*;

type AntiBloatReservationContextRow = (
    Uuid,
    Uuid,
    i64,
    String,
    String,
    i64,
    i64,
    i64,
    Option<String>,
    Option<i64>,
    Option<i32>,
);
type AntiBloatConsumptionRow = (Option<i64>, Option<i64>, Option<i64>, bool);

pub(super) async fn begin_send(
    uow: &mut PgUnitOfWork,
    saved: &StoredAntiBloatReview,
    prepared: &AntiBloatPreparedRequest,
    policy: &AdvisoryBudgetPolicy,
    required_profile: Option<&str>,
) -> Result<Option<AntiBloatSendPermit>> {
    if !uow.is_read_write() || uow.principal_id()? != saved.actor_id {
        return Err(Error::Forbidden);
    }
    let tenant = uow.tenant_id()?;
    if let Some(profile) = required_profile
        && !input::provider_profile_matches(uow, saved.workspace_id, profile).await?
    {
        response::record_preflight_no_call(
            uow,
            saved.review_id,
            tect_application::AntiBloatNoCall::PreflightInvalidConfiguration,
        )
        .await?;
        return Ok(None);
    }
    let locked_revision: Option<i64> = sqlx::query_scalar(
        "SELECT revision FROM scope_candidate_sets WHERE tenant_id=$1 \
         AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(saved.workspace_id)
    .bind(saved.review.candidate_set_id)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    if locked_revision != Some(saved.review.plan_revision) {
        return Err(Error::InputConflict);
    }
    if uow
        .authoritative_input(
            saved.workspace_id,
            saved.review.candidate_set_id,
            saved.review.plan_revision,
        )
        .await?
        .as_ref()
        != Some(&saved.input)
    {
        return Err(Error::InputConflict);
    }
    let eligible = saved
        .review
        .findings
        .iter()
        .filter(|item| item.rankable)
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    if eligible.is_empty() || saved.state != AntiBloatAttemptState::Prepared {
        return Ok(None);
    }
    if digest(&prepared.bytes) != prepared.sha256 {
        return Err(Error::InputConflict);
    }
    if prepared.material_sha256 != tect_application::anti_bloat_material_sha256(saved)?
        || prepared.adapter_identity.is_empty()
    {
        return Err(Error::InputConflict);
    }
    policy.validate().map_err(|_| Error::BudgetPolicyInvalid)?;
    let request_len =
        i64::try_from(prepared.bytes.len()).map_err(|_| Error::BudgetExhaustedBeforeDispatch)?;
    let ceilings = policy.ceilings();
    if request_len == 0 || request_len > ceilings.request_utf8_bytes || ceilings.provider_calls < 1
    {
        return Err(Error::BudgetExhaustedBeforeDispatch);
    }
    // The authorization seam is trusted by contract; reservation checks the
    // installed row after the opportunity and workspace policy locks.
    let opportunity_id: Uuid = sqlx::query_scalar(
        "SELECT b.opportunity_id FROM scope_anti_bloat_bindings b WHERE \
         b.tenant_id=$1 AND b.workspace_id=$2 AND b.candidate_set_id=$3 \
         AND b.candidate_set_revision=$4",
    )
    .bind(tenant)
    .bind(saved.workspace_id)
    .bind(saved.review.candidate_set_id)
    .bind(saved.review.plan_revision)
    .fetch_one(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    let locked: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM advisory_opportunity WHERE id=$1 AND tenant_id=$2 \
         AND workspace_id=$3 FOR UPDATE",
    )
    .bind(opportunity_id)
    .bind(tenant)
    .bind(saved.workspace_id)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    if locked != Some(opportunity_id) {
        return Err(Error::InputConflict);
    }
    crate::budget_policy_usage::lock_workspace_policy(
        uow.transaction()?,
        tenant,
        saved.workspace_id,
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
    let usage = crate::budget_policy_usage::policy_usage(
        uow.transaction()?,
        tenant,
        saved.workspace_id,
        policy.id(),
        policy.version(),
        policy.digest(),
    )
    .await?;
    crate::budget_policy_usage::require_current_policy(
        uow.transaction()?,
        tenant,
        saved.workspace_id,
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
    let remaining_input = ceilings
        .input_tokens
        .checked_sub(usage.input_tokens)
        .ok_or(Error::BudgetExhaustedBeforeDispatch)?;
    let remaining_output = ceilings
        .output_tokens
        .checked_sub(usage.output_tokens)
        .ok_or(Error::BudgetExhaustedBeforeDispatch)?;
    let remaining_elapsed = ceilings
        .elapsed_monotonic_ms
        .checked_sub(usage.elapsed_ms)
        .ok_or(Error::BudgetExhaustedBeforeDispatch)?;
    if remaining_input <= 0 || remaining_output <= 0 || remaining_elapsed <= 0 {
        return Err(Error::BudgetExhaustedBeforeDispatch);
    }
    let result = sqlx::query(
        "UPDATE scope_anti_bloat_reviews SET state='sending',request_bytes=$5, \
         request_sha256=$6,request_adapter_identity=$9,send_started_at=pg_catalog.clock_timestamp() \
         WHERE tenant_id=$1 AND workspace_id=$2 AND review_id=$3 AND actor_id=$4 \
           AND state='prepared' AND input_payload=$7 AND review_payload=$8",
    )
    .bind(tenant)
    .bind(saved.workspace_id)
    .bind(saved.review_id)
    .bind(saved.actor_id)
    .bind(&prepared.bytes)
    .bind(&prepared.sha256)
    .bind(serde_json::to_value(&saved.input).map_err(storage_error)?)
    .bind(serde_json::to_value(&saved.review).map_err(storage_error)?)
    .bind(&prepared.adapter_identity)
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    if result.rows_affected() == 1 {
        sqlx::query(
            "INSERT INTO scope_anti_bloat_budget_reservations \
             (tenant_id,workspace_id,opportunity_id,review_id,policy_id,policy_version,policy_digest,\
             request_sha256,request_utf8_bytes,reserved_input_tokens,reserved_output_tokens,\
             reserved_elapsed_ms) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(tenant)
        .bind(saved.workspace_id)
        .bind(opportunity_id)
        .bind(saved.review_id)
        .bind(policy.id())
        .bind(policy.version())
        .bind(policy.digest())
        .bind(&prepared.sha256)
        .bind(request_len)
        .bind(remaining_input)
        .bind(remaining_output)
        .bind(remaining_elapsed)
        .execute(&mut **uow.transaction()?)
        .await
        .map_err(storage_error)?;
    }
    Ok((result.rows_affected() == 1).then(|| AntiBloatSendPermit {
        review_id: saved.review_id,
        request: prepared.clone(),
    }))
}

pub(super) async fn authorized_sealed_response(
    uow: &mut PgUnitOfWork,
    permit: &AntiBloatSendPermit,
) -> Result<AntiBloatProviderObservation> {
    let observation = super::response::sealed_response_for_usage(uow, permit).await?;
    super::response::validate_permit_material(uow, permit).await?;
    let raw: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT v.raw_response FROM scope_anti_bloat_reviews v \
         JOIN scope_anti_bloat_budget_consumptions c ON \
         (v.tenant_id,v.workspace_id,v.review_id)=(c.tenant_id,c.workspace_id,c.review_id) \
         WHERE v.tenant_id=$1 AND v.review_id=$2 AND v.actor_id=$3 \
         AND v.state='sending' AND v.response_sealed_at IS NOT NULL \
         AND v.request_bytes=$4 AND v.request_sha256=$5 AND v.request_adapter_identity=$6 \
         AND c.request_sha256=v.request_sha256 AND c.response_sha256=v.response_sha256 \
         AND NOT c.unknown_usage AND NOT c.exhausted_after_response AND NOT c.transport_failed",
    )
    .bind(uow.tenant_id()?)
    .bind(permit.review_id)
    .bind(uow.principal_id()?)
    .bind(&permit.request.bytes)
    .bind(&permit.request.sha256)
    .bind(&permit.request.adapter_identity)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    raw.ok_or(Error::InputConflict)?;
    Ok(observation)
}

pub(super) async fn mark_send_unknown(uow: &mut PgUnitOfWork, review_id: Uuid) -> Result<()> {
    let tenant = uow.tenant_id()?;
    let actor = uow.principal_id()?;
    let changed = sqlx::query(
        "UPDATE scope_anti_bloat_reviews SET state='send_unknown' \
                 WHERE tenant_id=$1 AND review_id=$2 AND actor_id=$3 AND state='sending'",
    )
    .bind(tenant)
    .bind(review_id)
    .bind(actor)
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    if changed.rows_affected() == 1 {
        // A transport failure has no response bytes to seal. Its terminal
        // unknown observation is recorded once, after the review audit state.
        sqlx::query(
            "INSERT INTO scope_anti_bloat_budget_consumptions \
             (tenant_id,workspace_id,review_id,policy_id,policy_version,policy_digest,\
              request_sha256,response_sha256,input_tokens,output_tokens,elapsed_monotonic_ms,\
              unknown_usage,exhausted_after_response,transport_failed) \
             SELECT r.tenant_id,r.workspace_id,r.review_id,r.policy_id,r.policy_version,\
              r.policy_digest,r.request_sha256,NULL,NULL,NULL,NULL,true,true,true \
             FROM scope_anti_bloat_budget_reservations r JOIN scope_anti_bloat_reviews v \
             ON (v.tenant_id,v.workspace_id,v.review_id)=(r.tenant_id,r.workspace_id,r.review_id) \
             WHERE r.tenant_id=$1 AND r.review_id=$2 AND v.raw_response IS NULL \
             ON CONFLICT (tenant_id,workspace_id,review_id) DO NOTHING",
        )
        .bind(tenant)
        .bind(review_id)
        .execute(&mut **uow.transaction()?)
        .await
        .map_err(storage_error)?;
    }
    Ok(())
}

pub(super) async fn consume_budget(
    uow: &mut PgUnitOfWork,
    permit: &AntiBloatSendPermit,
    observation: &AntiBloatProviderObservation,
) -> Result<bool> {
    if !uow.is_read_write() {
        return Err(Error::Forbidden);
    }
    let tenant = uow.tenant_id()?;
    let actor = uow.principal_id()?;
    let row: Option<AntiBloatReservationContextRow> = sqlx::query_as(
        "SELECT r.workspace_id,r.policy_id,r.policy_version,r.policy_digest,r.request_sha256,\
         r.reserved_input_tokens,r.reserved_output_tokens,r.reserved_elapsed_ms,\
         v.response_sha256,v.response_original_elapsed_ms,v.response_http_status FROM scope_anti_bloat_budget_reservations r \
         JOIN scope_anti_bloat_reviews v ON \
         (v.tenant_id,v.workspace_id,v.review_id)=(r.tenant_id,r.workspace_id,r.review_id) \
         WHERE r.tenant_id=$1 AND r.review_id=$2 AND v.actor_id=$3 \
         AND v.state='sending' AND v.response_sealed_at IS NOT NULL \
         AND v.request_sha256=$4 AND v.request_bytes=$5 FOR UPDATE OF v",
    )
    .bind(tenant)
    .bind(permit.review_id)
    .bind(actor)
    .bind(&permit.request.sha256)
    .bind(&permit.request.bytes)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    let (
        workspace_id,
        policy_id,
        version,
        policy_digest,
        request_sha256,
        input_limit,
        output_limit,
        elapsed_limit,
        response_sha256,
        original_elapsed,
        http_status,
    ) = row.ok_or(Error::InputConflict)?;
    if response_sha256.as_deref() != Some(digest(&observation.raw).as_str()) {
        return Err(Error::InputConflict);
    }
    if observation.elapsed_monotonic_ms != original_elapsed
        || observation.http_status.map(i32::from) != http_status
    {
        return Err(Error::InputConflict);
    }
    let existing: Option<AntiBloatConsumptionRow> = sqlx::query_as(
        "SELECT input_tokens,output_tokens,elapsed_monotonic_ms,exhausted_after_response \
         FROM scope_anti_bloat_budget_consumptions WHERE tenant_id=$1 AND review_id=$2",
    )
    .bind(tenant)
    .bind(permit.review_id)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    if let Some(existing) = existing {
        if existing.0 != observation.input_tokens
            || existing.1 != observation.output_tokens
            || existing.2 != observation.elapsed_monotonic_ms
        {
            return Err(Error::InputConflict);
        }
        return Ok(existing.3);
    }
    let usage = crate::budget_policy_usage::policy_usage(
        uow.transaction()?,
        tenant,
        workspace_id,
        policy_id,
        version,
        &policy_digest,
    )
    .await?;
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
        "INSERT INTO scope_anti_bloat_budget_consumptions \
         (tenant_id,workspace_id,review_id,policy_id,policy_version,policy_digest,\
          request_sha256,response_sha256,input_tokens,output_tokens,elapsed_monotonic_ms,\
          unknown_usage,exhausted_after_response,transport_failed) \
         SELECT tenant_id,workspace_id,review_id,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,false \
         FROM scope_anti_bloat_reviews WHERE tenant_id=$1 AND review_id=$2",
    )
    .bind(tenant)
    .bind(permit.review_id)
    .bind(policy_id)
    .bind(version)
    .bind(policy_digest)
    .bind(request_sha256)
    .bind(response_sha256)
    .bind(observation.input_tokens)
    .bind(observation.output_tokens)
    .bind(observation.elapsed_monotonic_ms)
    .bind(unknown)
    .bind(exhausted)
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    Ok(exhausted)
}

pub(super) async fn seal_response(
    uow: &mut PgUnitOfWork,
    permit: &AntiBloatSendPermit,
    observation: &AntiBloatProviderObservation,
    response_sha256: &str,
) -> Result<()> {
    if !uow.is_read_write()
        || digest(&observation.raw) != response_sha256
        || observation
            .http_status
            .is_some_and(|n| !(100..=599).contains(&n))
    {
        return Err(Error::InputConflict);
    }
    let result = sqlx::query(
        "UPDATE scope_anti_bloat_reviews SET raw_response=$5,response_sha256=$6, \
         response_sealed_at=pg_catalog.clock_timestamp(), response_http_status=$8, \
         response_original_input_tokens=$9,response_original_output_tokens=$10,response_original_elapsed_ms=$11 \
         WHERE tenant_id=$1 AND review_id=$2 AND actor_id=$3 AND state='sending' \
           AND request_bytes=$4 AND request_sha256=$7 AND raw_response IS NULL",
    )
    .bind(uow.tenant_id()?)
    .bind(permit.review_id)
    .bind(uow.principal_id()?)
    .bind(&permit.request.bytes)
    .bind(&observation.raw)
    .bind(response_sha256)
    .bind(&permit.request.sha256)
    .bind(observation.http_status.map(i32::from))
    .bind(observation.input_tokens)
    .bind(observation.output_tokens)
    .bind(observation.elapsed_monotonic_ms)
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    if result.rows_affected() != 1 {
        return Err(Error::InputConflict);
    }
    Ok(())
}

pub(super) async fn seal_ranked(
    uow: &mut PgUnitOfWork,
    review_id: Uuid,
    ranked_ids: &[String],
) -> Result<()> {
    let tenant = uow.tenant_id()?;
    let actor = uow.principal_id()?;
    let value = serde_json::to_value(ranked_ids).map_err(storage_error)?;
    let result = sqlx::query(
        "UPDATE scope_anti_bloat_reviews SET state='ranked',ranked_ids=$4, \
         sealed_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND review_id=$2 \
         AND actor_id=$3 AND state='sending' AND raw_response IS NOT NULL \
         AND (response_http_status IS NULL OR response_http_status BETWEEN 200 AND 299) \
         AND eligible_ids @> $4::jsonb AND $4::jsonb @> eligible_ids \
         AND EXISTS (SELECT 1 FROM scope_anti_bloat_budget_consumptions c \
             WHERE (c.tenant_id,c.workspace_id,c.review_id)= \
               (scope_anti_bloat_reviews.tenant_id,scope_anti_bloat_reviews.workspace_id,scope_anti_bloat_reviews.review_id) \
               AND NOT c.unknown_usage AND NOT c.exhausted_after_response)",
    )
    .bind(tenant)
    .bind(review_id)
    .bind(actor)
    .bind(value)
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    if result.rows_affected() != 1 {
        return Err(Error::InputConflict);
    }
    Ok(())
}
