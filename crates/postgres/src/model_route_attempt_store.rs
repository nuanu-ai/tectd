//! Route-specific one-use optional-adviser ledger; no model execution path.
use async_trait::async_trait;
use serde_json::Value;
use sqlx::Row;
use tect_application::{
    ModelRouteAttemptSnapshot, ModelRouteAttemptState, ModelRouteAttemptStore,
    ModelRouteInvocation, ModelRoutePreparedAttempt, ModelRouteProviderObservation,
    ModelRouteRecommendationStore, ModelRouteRunNoCall, ModelRouteSealedRankingEvidence,
    ModelRouteSendPermit, PreparedModelRouteRecommendation,
};
use tect_domain::{
    AdvisoryBudgetPolicy, Error, ModelRouteRankingWireRequest, Result, model_route_wire_sha256,
    parse_model_route_ranking_response,
};
use uuid::Uuid;

use crate::{model_route_store::current_preparation, storage_error, store::PgUnitOfWork};
mod reads;
use reads::row_permit;
pub(crate) use reads::{audit_state, sealed_ranking};

fn write_error(error: sqlx::Error) -> Error {
    match error.as_database_error().and_then(|error| error.code()) {
        Some(code) if code == "42501" => Error::Forbidden,
        Some(code) if code == "23505" || code == "23514" || code == "23503" => Error::InputConflict,
        _ => storage_error(error),
    }
}

fn no_call_reason(reason: ModelRouteRunNoCall) -> Result<&'static str> {
    use tect_application::ModelRoutePreparation::*;
    Ok(match reason {
        ModelRouteRunNoCall::ProviderUnavailable => "provider_unavailable",
        ModelRouteRunNoCall::Preparation(WorkspaceDisabled) => "workspace_disabled",
        ModelRouteRunNoCall::Preparation(SessionSkip) => "session_skip",
        ModelRouteRunNoCall::Preparation(RequestSkip) => "request_skip",
        ModelRouteRunNoCall::Preparation(CapabilityUnavailable) => "capability_unavailable",
        ModelRouteRunNoCall::Preparation(UnknownWorkFacts) => "unknown_work_facts",
        ModelRouteRunNoCall::Preparation(NoEligibleRoutes) => "no_eligible_routes",
        ModelRouteRunNoCall::Preparation(Prepared) => return Err(Error::InputConflict),
    })
}

async fn stored_preparation(
    uow: &mut PgUnitOfWork,
    prepared: &PreparedModelRouteRecommendation,
) -> Result<()> {
    let stored = uow
        .by_request(prepared.workspace_id, &prepared.request_key)
        .await?
        .ok_or(Error::StaleContext)?;
    if stored != *prepared {
        return Err(Error::InputConflict);
    }
    current_preparation(uow, &stored).await?;
    Ok(())
}

async fn attempt_row(
    uow: &mut PgUnitOfWork,
    workspace_id: Uuid,
    request_key: &str,
) -> Result<Option<sqlx::postgres::PgRow>> {
    let tenant = uow.tenant_id()?;
    sqlx::query(
        "SELECT a.id,a.invoking_session_id,a.invoking_principal_id,a.state,a.no_call_reason,a.adviser_model, \
         a.request_payload,a.request_sha256,a.response_payload,a.response_sha256,a.parsed_outcome, \
         r.policy_id,r.policy_version,r.policy_digest,c.exhausted_after_response \
         FROM model_route_advisory_attempts a LEFT JOIN model_route_budget_reservations r \
         ON (r.tenant_id,r.workspace_id,r.attempt_id)=(a.tenant_id,a.workspace_id,a.id) \
         LEFT JOIN model_route_budget_consumptions c ON \
         (c.tenant_id,c.workspace_id,c.attempt_id)=(a.tenant_id,a.workspace_id,a.id) \
         WHERE a.tenant_id=$1 AND a.workspace_id=$2 AND a.preparation_request_key=$3",
    )
    .bind(tenant)
    .bind(workspace_id)
    .bind(request_key)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)
}

async fn authenticated_invocation(
    uow: &mut PgUnitOfWork,
    _workspace_id: Uuid,
    invocation: ModelRouteInvocation,
) -> Result<Uuid> {
    if invocation.session_id.is_nil() || !uow.is_read_write() {
        return Err(Error::Forbidden);
    }
    let principal = uow.principal_id()?;
    // Runtime has no direct private host read. The SECURITY DEFINER insert
    // trigger proves the supplied session is live and belongs to this actor.
    Ok(principal)
}

async fn insert_attempt(
    uow: &mut PgUnitOfWork,
    prepared: &PreparedModelRouteRecommendation,
    invocation: ModelRouteInvocation,
    principal: Uuid,
    state: &str,
    reason: Option<&str>,
    attempted: Option<&ModelRoutePreparedAttempt>,
) -> Result<Uuid> {
    let tenant = uow.tenant_id()?;
    let id = Uuid::new_v4();
    let work = &prepared.work;
    sqlx::query(
        "INSERT INTO model_route_advisory_attempts \
         (tenant_id,workspace_id,id,preparation_request_key,invoking_session_id, \
          invoking_principal_id,candidate_set_id,work_node_id,work_node_revision, \
          task_id,task_revision,work_digest,catalogue_digest,host_capability_evidence_ref, \
          state,no_call_reason,adviser_model,request_payload,request_sha256) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)",
    )
    .bind(tenant)
    .bind(prepared.workspace_id)
    .bind(id)
    .bind(&prepared.request_key)
    .bind(invocation.session_id)
    .bind(principal)
    .bind(work.selection_link.candidate_set_id)
    .bind(work.selection_link.mapped_work_node_id)
    .bind(work.selection_link.mapped_work_node_revision)
    .bind(work.approved_matrix_selection.task_id)
    .bind(work.approved_matrix_selection.task_revision)
    .bind(work.digest()?)
    .bind(
        prepared
            .catalogue
            .as_ref()
            .map(|c| c.digest())
            .transpose()?,
    )
    .bind(match &work.host_capabilities {
        tect_domain::ModelRouteFact::Known {
            provenance: tect_domain::ModelRouteFactProvenance::Host { evidence_ref },
            ..
        } => Some(evidence_ref.as_str()),
        _ => None,
    })
    .bind(state)
    .bind(reason)
    .bind(attempted.map(|a| a.request.binding.adviser_model.as_str()))
    .bind(attempted.map(|a| a.request_bytes.as_slice()))
    .bind(attempted.map(|a| a.request_sha256.as_str()))
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(write_error)?;
    Ok(id)
}

async fn permit_row(
    uow: &mut PgUnitOfWork,
    permit: &ModelRouteSendPermit,
) -> Result<sqlx::postgres::PgRow> {
    let row = attempt_row(uow, permit.workspace_id, &permit.preparation_request_key)
        .await?
        .ok_or(Error::StaleContext)?;
    if row.try_get::<Uuid, _>("id").map_err(storage_error)? != permit.attempt_id
        || row
            .try_get::<Option<String>, _>("request_sha256")
            .map_err(storage_error)?
            != Some(permit.request_sha256.clone())
        || row
            .try_get::<Option<Uuid>, _>("policy_id")
            .map_err(storage_error)?
            != Some(permit.policy_id)
        || row
            .try_get::<Option<i64>, _>("policy_version")
            .map_err(storage_error)?
            != Some(permit.policy_version)
        || row
            .try_get::<Option<String>, _>("policy_digest")
            .map_err(storage_error)?
            != Some(permit.policy_digest.clone())
    {
        return Err(Error::InputConflict);
    }
    Ok(row)
}

#[async_trait]
impl ModelRouteAttemptStore for PgUnitOfWork {
    async fn by_preparation(
        &mut self,
        workspace_id: Uuid,
        preparation_request_key: &str,
        invocation: ModelRouteInvocation,
    ) -> Result<Option<ModelRouteAttemptSnapshot>> {
        let principal = self.principal_id()?;
        let Some(row) = attempt_row(self, workspace_id, preparation_request_key).await? else {
            return Ok(None);
        };
        if row
            .try_get::<Uuid, _>("invoking_session_id")
            .map_err(storage_error)?
            != invocation.session_id
            || row
                .try_get::<Uuid, _>("invoking_principal_id")
                .map_err(storage_error)?
                != principal
        {
            return Err(Error::Forbidden);
        }
        let state: String = row.try_get("state").map_err(storage_error)?;
        let state = match state.as_str() {
            "no_call" => ModelRouteAttemptState::NoCall,
            "send_unknown" => ModelRouteAttemptState::SendUnknown,
            "raw_sealed"
                if row
                    .try_get::<Option<bool>, _>("exhausted_after_response")
                    .map_err(storage_error)?
                    == Some(true) =>
            {
                ModelRouteAttemptState::BudgetExhausted
            }
            "raw_sealed" => ModelRouteAttemptState::RawSealed,
            "parsed" => ModelRouteAttemptState::Parsed,
            _ => return Err(Error::StorageUnavailable),
        };
        Ok(Some(ModelRouteAttemptSnapshot {
            attempt_id: row.try_get("id").map_err(storage_error)?,
            state,
            no_call_reason: row.try_get("no_call_reason").map_err(storage_error)?,
            request_sha256: row.try_get("request_sha256").map_err(storage_error)?,
            response_sha256: row.try_get("response_sha256").map_err(storage_error)?,
        }))
    }

    async fn recover_raw_sealed(
        &mut self,
        workspace_id: Uuid,
        preparation_request_key: &str,
        invocation: ModelRouteInvocation,
    ) -> Result<Option<(ModelRoutePreparedAttempt, ModelRouteSendPermit)>> {
        let prepared = self
            .by_request(workspace_id, preparation_request_key)
            .await?
            .ok_or(Error::StaleContext)?;
        current_preparation(self, &prepared).await?;
        let principal = self.principal_id()?;
        let Some(row) = attempt_row(self, workspace_id, preparation_request_key).await? else {
            return Ok(None);
        };
        if row
            .try_get::<Uuid, _>("invoking_session_id")
            .map_err(storage_error)?
            != invocation.session_id
            || row
                .try_get::<Uuid, _>("invoking_principal_id")
                .map_err(storage_error)?
                != principal
        {
            return Err(Error::Forbidden);
        }
        if row.try_get::<String, _>("state").map_err(storage_error)? != "raw_sealed" {
            return Ok(None);
        }
        let request_bytes: Vec<u8> = row.try_get("request_payload").map_err(storage_error)?;
        let request: ModelRouteRankingWireRequest =
            serde_json::from_slice(&request_bytes).map_err(|_| Error::InputConflict)?;
        let attempted = ModelRoutePreparedAttempt {
            request,
            request_sha256: row.try_get("request_sha256").map_err(storage_error)?,
            request_bytes,
        };
        attempted.verify(&prepared)?;
        Ok(Some((
            attempted,
            row_permit(&row, workspace_id, preparation_request_key)?,
        )))
    }
    async fn record_no_call(
        &mut self,
        prepared: &PreparedModelRouteRecommendation,
        invocation: ModelRouteInvocation,
        reason: ModelRouteRunNoCall,
    ) -> Result<()> {
        stored_preparation(self, prepared).await?;
        let principal = authenticated_invocation(self, prepared.workspace_id, invocation).await?;
        let reason_text = no_call_reason(reason)?;
        if !matches!(reason, ModelRouteRunNoCall::ProviderUnavailable)
            && reason != ModelRouteRunNoCall::Preparation(prepared.preparation)
            || matches!(reason, ModelRouteRunNoCall::ProviderUnavailable)
                && prepared.preparation != tect_application::ModelRoutePreparation::Prepared
        {
            return Err(Error::InputConflict);
        }
        if let Some(row) = attempt_row(self, prepared.workspace_id, &prepared.request_key).await? {
            return if row.try_get::<String, _>("state").map_err(storage_error)? == "no_call"
                && row
                    .try_get::<Option<String>, _>("no_call_reason")
                    .map_err(storage_error)?
                    == Some(reason_text.into())
                && row
                    .try_get::<Uuid, _>("invoking_session_id")
                    .map_err(storage_error)?
                    == invocation.session_id
                && row
                    .try_get::<Uuid, _>("invoking_principal_id")
                    .map_err(storage_error)?
                    == principal
            {
                Ok(())
            } else {
                Err(Error::InputConflict)
            };
        }
        insert_attempt(
            self,
            prepared,
            invocation,
            principal,
            "no_call",
            Some(reason_text),
            None,
        )
        .await?;
        Ok(())
    }

    async fn begin_send(
        &mut self,
        prepared: &PreparedModelRouteRecommendation,
        invocation: ModelRouteInvocation,
        attempted: &ModelRoutePreparedAttempt,
        policy: &AdvisoryBudgetPolicy,
    ) -> Result<Option<ModelRouteSendPermit>> {
        stored_preparation(self, prepared).await?;
        if prepared.preparation != tect_application::ModelRoutePreparation::Prepared {
            return Err(Error::InputConflict);
        }
        attempted.verify(prepared)?;
        let principal = authenticated_invocation(self, prepared.workspace_id, invocation).await?;
        if let Some(row) = attempt_row(self, prepared.workspace_id, &prepared.request_key).await? {
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
        let tenant = self.tenant_id()?;
        let now: i64 = sqlx::query_scalar(
            "SELECT (EXTRACT(EPOCH FROM pg_catalog.clock_timestamp())*1000)::bigint",
        )
        .fetch_one(&mut **self.transaction()?)
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
            self.transaction()?,
            tenant,
            prepared.workspace_id,
            policy.id(),
            policy.version(),
            policy.digest(),
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
            self,
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
        .execute(&mut **self.transaction()?)
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

    async fn seal_raw_response(
        &mut self,
        permit: &ModelRouteSendPermit,
        raw: &[u8],
        digest: &str,
    ) -> Result<()> {
        if !self.is_read_write() || model_route_wire_sha256(raw) != digest {
            return Err(Error::InputConflict);
        }
        let row = permit_row(self, permit).await?;
        let state: String = row.try_get("state").map_err(storage_error)?;
        if state == "raw_sealed" || state == "parsed" {
            return if row
                .try_get::<Option<Vec<u8>>, _>("response_payload")
                .map_err(storage_error)?
                == Some(raw.to_vec())
                && row
                    .try_get::<Option<String>, _>("response_sha256")
                    .map_err(storage_error)?
                    == Some(digest.into())
            {
                Ok(())
            } else {
                Err(Error::InputConflict)
            };
        }
        if state != "send_unknown" {
            return Err(Error::InputConflict);
        }
        let tenant = self.tenant_id()?;
        let affected = sqlx::query(
            "UPDATE model_route_advisory_attempts SET state='raw_sealed',response_payload=$5, \
             response_sha256=$6,raw_sealed_at=pg_catalog.clock_timestamp() \
             WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND request_sha256=$4 \
             AND state='send_unknown'",
        )
        .bind(tenant)
        .bind(permit.workspace_id)
        .bind(permit.attempt_id)
        .bind(&permit.request_sha256)
        .bind(raw)
        .bind(digest)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(write_error)?
        .rows_affected();
        if affected != 1 {
            return Err(Error::InputConflict);
        }
        Ok(())
    }

    async fn sealed_response(&mut self, permit: &ModelRouteSendPermit) -> Result<Option<Vec<u8>>> {
        let row = permit_row(self, permit).await?;
        let state: String = row.try_get("state").map_err(storage_error)?;
        if state != "raw_sealed" && state != "parsed" {
            return Ok(None);
        }
        let raw: Option<Vec<u8>> = row.try_get("response_payload").map_err(storage_error)?;
        let digest: Option<String> = row.try_get("response_sha256").map_err(storage_error)?;
        if raw.as_ref().map(|raw| model_route_wire_sha256(raw)) != digest {
            return Err(Error::InputConflict);
        }
        Ok(raw)
    }

    async fn consume_budget(
        &mut self,
        permit: &ModelRouteSendPermit,
        observation: &ModelRouteProviderObservation,
    ) -> Result<bool> {
        if !self.is_read_write() {
            return Err(Error::Forbidden);
        }
        let row = permit_row(self, permit).await?;
        let state: String = row.try_get("state").map_err(storage_error)?;
        if state != "raw_sealed" && state != "parsed" {
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
        let tenant = self.tenant_id()?;
        let existing: Option<(Option<i64>,Option<i64>,Option<i64>,bool)> = sqlx::query_as(
            "SELECT input_tokens,output_tokens,elapsed_monotonic_ms,exhausted_after_response \
             FROM model_route_budget_consumptions WHERE tenant_id=$1 AND workspace_id=$2 AND attempt_id=$3",
        ).bind(tenant).bind(permit.workspace_id).bind(permit.attempt_id)
        .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        if let Some(existing) = existing {
            if existing.0 != observation.input_tokens
                || existing.1 != observation.output_tokens
                || existing.2 != observation.elapsed_monotonic_ms
            {
                return Err(Error::InputConflict);
            }
            return Ok(existing.3);
        }
        let limits: Option<(i64,i64,i64)> = sqlx::query_as(
            "SELECT reserved_input_tokens,reserved_output_tokens,reserved_elapsed_ms \
             FROM model_route_budget_reservations WHERE tenant_id=$1 AND workspace_id=$2 AND attempt_id=$3",
        ).bind(tenant).bind(permit.workspace_id).bind(permit.attempt_id)
        .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        let (input_limit, output_limit, elapsed_limit) = limits.ok_or(Error::InputConflict)?;
        let usage = crate::budget_policy_usage::policy_usage(
            self.transaction()?,
            tenant,
            permit.workspace_id,
            permit.policy_id,
            permit.policy_version,
            &permit.policy_digest,
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
        .execute(&mut **self.transaction()?)
        .await
        .map_err(write_error)?;
        Ok(exhausted)
    }

    async fn consumption_healthy(&mut self, permit: &ModelRouteSendPermit) -> Result<Option<bool>> {
        permit_row(self, permit).await?;
        let tenant = self.tenant_id()?;
        sqlx::query_scalar(
            "SELECT NOT unknown_usage AND NOT exhausted_after_response \
             FROM model_route_budget_consumptions WHERE tenant_id=$1 AND workspace_id=$2 AND attempt_id=$3",
        ).bind(tenant).bind(permit.workspace_id).bind(permit.attempt_id)
        .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)
    }

    async fn capture_sealed_outcome(
        &mut self,
        evidence: &ModelRouteSealedRankingEvidence,
    ) -> Result<()> {
        if !self.is_read_write() {
            return Err(Error::Forbidden);
        }
        let prepared = self
            .by_request(
                evidence.permit.workspace_id,
                &evidence.permit.preparation_request_key,
            )
            .await?
            .ok_or(Error::StaleContext)?;
        current_preparation(self, &prepared).await?;
        evidence.verify(&prepared)?;
        let row = permit_row(self, &evidence.permit).await?;
        if row
            .try_get::<Option<Vec<u8>>, _>("request_payload")
            .map_err(storage_error)?
            != Some(evidence.attempted.request_bytes.clone())
            || row
                .try_get::<Option<Vec<u8>>, _>("response_payload")
                .map_err(storage_error)?
                != Some(evidence.raw_response.clone())
        {
            return Err(Error::InputConflict);
        }
        let outcome = serde_json::to_value(&evidence.outcome).map_err(storage_error)?;
        let state: String = row.try_get("state").map_err(storage_error)?;
        if state == "parsed" {
            return if row
                .try_get::<Option<Value>, _>("parsed_outcome")
                .map_err(storage_error)?
                == Some(outcome)
            {
                Ok(())
            } else {
                Err(Error::InputConflict)
            };
        }
        if state != "raw_sealed" {
            return Err(Error::InputConflict);
        }
        let tenant = self.tenant_id()?;
        let healthy: Option<bool> = sqlx::query_scalar(
            "SELECT NOT unknown_usage AND NOT exhausted_after_response \
             FROM model_route_budget_consumptions WHERE tenant_id=$1 AND workspace_id=$2 AND attempt_id=$3",
        ).bind(tenant).bind(evidence.permit.workspace_id).bind(evidence.permit.attempt_id)
        .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        if healthy != Some(true) {
            return Err(Error::BudgetPolicyInvalid);
        }
        let affected = sqlx::query(
            "UPDATE model_route_advisory_attempts SET state='parsed',parsed_outcome=$4, \
             parsed_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 \
             AND id=$3 AND state='raw_sealed'",
        )
        .bind(tenant)
        .bind(evidence.permit.workspace_id)
        .bind(evidence.permit.attempt_id)
        .bind(outcome)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(write_error)?
        .rows_affected();
        if affected != 1 {
            return Err(Error::InputConflict);
        }
        Ok(())
    }

    async fn mark_send_unknown(&mut self, permit: &ModelRouteSendPermit) -> Result<()> {
        let row = permit_row(self, permit).await?;
        let state: String = row.try_get("state").map_err(storage_error)?;
        if state != "send_unknown" {
            return Err(Error::InputConflict);
        }
        let tenant = self.tenant_id()?;
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
        .execute(&mut **self.transaction()?)
        .await
        .map_err(write_error)?;
        Ok(())
    }
}
