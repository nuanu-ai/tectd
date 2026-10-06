use super::*;

pub(super) async fn seal(
    uow: &mut PgUnitOfWork,
    permit: &ModelRouteSendPermit,
    observation: &ModelRouteProviderObservation,
) -> Result<()> {
    if !uow.is_read_write() {
        return Err(Error::Forbidden);
    }
    if let Some(context) = observation.original_transport_context.as_ref() {
        context.validate_for(&Some(observation.raw.clone()))?;
    }
    let row = permit_row(uow, permit).await?;
    if let Some(existing) = reads::observation_from_row(&row)? {
        return if existing == *observation {
            Ok(())
        } else {
            Err(Error::InputConflict)
        };
    }
    if row.try_get::<String, _>("state").map_err(storage_error)? != "send_unknown" {
        return Err(Error::InputConflict);
    }
    let tenant = uow.tenant_id()?;
    let affected = sqlx::query("UPDATE model_route_advisory_attempts SET state='raw_sealed',response_payload=$5,response_sha256=$6,raw_sealed_at=pg_catalog.clock_timestamp(),response_http_status=$7,response_original_input_tokens=$8,response_original_output_tokens=$9,response_original_elapsed_ms=$10,response_complete=$11,original_transport_context=$12 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND request_sha256=$4 AND state='send_unknown'")
        .bind(tenant).bind(permit.workspace_id).bind(permit.attempt_id).bind(&permit.request_sha256)
        .bind(&observation.raw).bind(model_route_wire_sha256(&observation.raw)).bind(observation.http_status.map(i32::from))
        .bind(observation.input_tokens).bind(observation.output_tokens).bind(observation.elapsed_monotonic_ms)
        .bind(observation.response_complete)
        .bind(observation.original_transport_context.as_ref().map(crate::advisory::encode_transport_context).transpose()?)
        .execute(&mut **uow.transaction()?).await.map_err(write_error)?.rows_affected();
    if affected != 1 {
        return Err(Error::InputConflict);
    }
    Ok(())
}

pub(super) async fn capture(
    uow: &mut PgUnitOfWork,
    evidence: &ModelRouteSealedRankingEvidence,
    provider: Option<&dyn tect_application::ModelRouteRankingProvider>,
) -> Result<()> {
    if !uow.is_read_write() {
        return Err(Error::Forbidden);
    }
    let prepared = uow
        .by_request(
            evidence.permit.workspace_id,
            &evidence.permit.preparation_request_key,
        )
        .await?
        .ok_or(Error::StaleContext)?;
    current_preparation(uow, &prepared).await?;
    if let Some(provider) = provider {
        if evidence.attempted.adapter_identity.is_some() && provider.required_profile().is_none() {
            return Err(Error::TransportUnavailable);
        }
        if let Some(profile) = provider.required_profile()
            && !uow.provider_profile_matches(&prepared, profile).await?
        {
            return Err(Error::TransportUnavailable);
        }
    }
    evidence.validate_material(&prepared)?;
    let row = permit_row(uow, &evidence.permit).await?;
    let tenant = uow.tenant_id()?;
    let healthy: Option<bool> = sqlx::query_scalar("SELECT NOT unknown_usage AND NOT exhausted_after_response FROM model_route_budget_consumptions WHERE tenant_id=$1 AND workspace_id=$2 AND attempt_id=$3")
        .bind(tenant).bind(evidence.permit.workspace_id).bind(evidence.permit.attempt_id).fetch_optional(&mut **uow.transaction()?).await.map_err(storage_error)?;
    if healthy != Some(true) {
        return Err(Error::BudgetPolicyInvalid);
    }
    if reads::row_attempted(&row)? != evidence.attempted {
        return Err(Error::InputConflict);
    }
    let observation = reads::observation_from_row(&row)?.ok_or(Error::StaleContext)?;
    if observation
        .http_status
        .is_some_and(|s| !(200..300).contains(&s))
    {
        return Err(Error::InvalidArguments);
    }
    if let Some(provider) = provider {
        if provider.parse_sealed(&evidence.attempted, &observation)? != evidence.outcome {
            return Err(Error::InputConflict);
        }
    } else {
        evidence.verify(&prepared)?;
    }
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
    let affected = sqlx::query(
        "UPDATE model_route_advisory_attempts SET state='parsed',parsed_outcome=$4, \
             parsed_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 \
             AND id=$3 AND state='raw_sealed'",
    )
    .bind(tenant)
    .bind(evidence.permit.workspace_id)
    .bind(evidence.permit.attempt_id)
    .bind(outcome)
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(write_error)?
    .rows_affected();
    if affected != 1 {
        return Err(Error::InputConflict);
    }
    Ok(())
}
