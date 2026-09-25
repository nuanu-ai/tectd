use tect_application::{
    MAX_PREPARED_PIPELINE_BODY_BYTES, MAX_SEALED_PIPELINE_RESPONSE_BYTES,
    PipelineDispatchCapability, PipelineProviderObservation, PipelineRecommendationDispatchStore,
    StoredPipelineRecommendationDispatch,
};

fn pipeline_digest(bytes: &[u8]) -> String {
    format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(bytes))
}

async fn pipeline_dispatch_row(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    dispatch_id: Uuid,
    for_update: bool,
) -> Result<DispatchRow> {
    let suffix = if for_update { " FOR UPDATE" } else { "" };
    let sql = format!(
        "SELECT {DISPATCH_COLUMNS} FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3{suffix}"
    );
    let row: DispatchRow = sqlx::query_as(&sql)
        .bind(tenant)
        .bind(workspace)
        .bind(dispatch_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(storage_error)?
        .ok_or(Error::NotFound)?;
    let capability: Option<String> = sqlx::query_scalar(
        "SELECT capability FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(row.opportunity_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if capability.as_deref() != Some("pipeline_recommendation") {
        return Err(Error::InputConflict);
    }
    Ok(row)
}

async fn authorize_pipeline(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    expected_config_revision: i64,
    input: &AdvisoryDispatchAuthorization,
) -> Result<AdvisoryDispatch> {
    input.validate()?;
    if input.attempt_number != 1
        || input.predecessor_dispatch_id.is_some()
        || input.retry_basis != AdvisoryRetryBasis::Initial
        || input.request_payload.len() > MAX_PREPARED_PIPELINE_BODY_BYTES
        || std::str::from_utf8(&input.request_payload).is_err()
        || input.payload_digest != pipeline_digest(&input.request_payload)
        || input.configuration_digest
            != pipeline_digest(
                &serde_json::to_vec(&input.configuration_snapshot)
                    .map_err(Error::invalid_arguments_from)?,
            )
        || input.configuration_snapshot.get("request_body_sha256")
            != Some(&serde_json::json!(input.payload_digest))
        || input
            .configuration_snapshot
            .get("destination")
            .and_then(|v| v.as_str())
            .is_none_or(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
        || input
            .configuration_snapshot
            .get("wire_version")
            .and_then(|v| v.as_str())
            .is_none_or(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
    {
        return Err(Error::InputConflict);
    }
    let opportunity = opportunity_by_id(tx, tenant, workspace, input.opportunity_id, true).await?;
    if opportunity.capability != AdvisoryCapability::PipelineRecommendation
        || opportunity.decision_point
            != AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen
        || opportunity.state != AdvisoryOpportunityState::Prepared
        || opportunity.primary_reason != AdvisoryReason::RecommendationPrepared
        || opportunity.material_digest != input.material_digest
        || opportunity.config_revision != expected_config_revision
    {
        return Err(Error::InputConflict);
    }
    let current: (i64, String, Option<String>, Option<serde_json::Value>) = sqlx::query_as(
        "SELECT revision,mode,provider_profile_ref,model_configuration \
         FROM advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2 FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if current.0 != expected_config_revision {
        return Err(Error::StaleRevision);
    }
    if current.1 != "optional"
        || current.2.as_deref()
            != input
                .configuration_snapshot
                .get("provider_profile_ref")
                .and_then(|v| v.as_str())
        || current
            .3
            .as_ref()
            .and_then(|v| v.get("model"))
            .and_then(|v| v.as_str())
            != Some(input.model.as_str())
    {
        return Err(Error::InvalidConfiguration);
    }
    let existing: Option<DispatchRow> = sqlx::query_as(&format!(
        "SELECT {DISPATCH_COLUMNS} FROM advisory_dispatch \
         WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3 FOR UPDATE"
    ))
    .bind(tenant)
    .bind(workspace)
    .bind(input.opportunity_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if let Some(row) = existing {
        if row.attempt_number != 1 || !dispatch_matches_authorization(&row, input)? {
            return Err(Error::InputConflict);
        }
        return dispatch_from_row(&row);
    }
    sqlx::query(
        "INSERT INTO advisory_dispatch \
         (id,tenant_id,workspace_id,opportunity_id,attempt_number,predecessor_dispatch_id,\
          provider,model,configuration_snapshot,configuration_digest,material_digest,\
          payload_digest,request_payload,state,send_certainty,retry_basis) \
         VALUES ($1,$2,$3,$4,1,NULL,$5,$6,$7,$8,$9,$10,$11,'authorized','not_sent','initial')",
    )
    .bind(input.dispatch_id)
    .bind(tenant)
    .bind(workspace)
    .bind(input.opportunity_id)
    .bind(&input.provider)
    .bind(&input.model)
    .bind(&input.configuration_snapshot)
    .bind(&input.configuration_digest)
    .bind(&input.material_digest)
    .bind(&input.payload_digest)
    .bind(&input.request_payload)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    let row = pipeline_dispatch_row(tx, tenant, workspace, input.dispatch_id, false).await?;
    dispatch_from_row(&row)
}

async fn start_pipeline(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    dispatch_id: Uuid,
) -> Result<AdvisoryDispatchStart> {
    let row = pipeline_dispatch_row(tx, tenant, workspace, dispatch_id, true).await?;
    if row.state != "authorized"
        || row.send_certainty != "not_sent"
        || row.attempt_number != 1
        || row.retry_basis != "initial"
    {
        return Err(Error::InputConflict);
    }
    let updated = sqlx::query(
        "UPDATE advisory_dispatch SET state='sending',send_certainty='sent_unknown',\
         send_started_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 \
         AND id=$3 AND state='authorized' AND send_certainty='not_sent'",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(dispatch_id)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    if updated.rows_affected() != 1 {
        return Err(Error::InputConflict);
    }
    let transitioned = sqlx::query(
        "UPDATE advisory_opportunity SET state='awaiting_response',primary_reason='send_unknown',\
         updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 \
         AND id=$3 AND capability='pipeline_recommendation' AND state='prepared' \
         AND primary_reason='recommendation_prepared'",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(row.opportunity_id)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    if transitioned.rows_affected() != 1 {
        return Err(Error::InputConflict);
    }
    let started = pipeline_dispatch_row(tx, tenant, workspace, dispatch_id, false).await?;
    Ok(AdvisoryDispatchStart {
        dispatch: dispatch_from_row(&started)?,
        should_send: true,
        budget_reservation: None,
    })
}

async fn seal_pipeline(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    dispatch_id: Uuid,
    observation: &PipelineProviderObservation,
) -> Result<StoredPipelineRecommendationDispatch> {
    if observation.raw_response.is_empty()
        || observation.raw_response.len() > MAX_SEALED_PIPELINE_RESPONSE_BYTES
    {
        return Err(Error::InvalidArguments);
    }
    let input_tokens = observation
        .input_tokens
        .map(i64::try_from)
        .transpose()
        .map_err(|_| Error::InvalidArguments)?;
    let output_tokens = observation
        .output_tokens
        .map(i64::try_from)
        .transpose()
        .map_err(|_| Error::InvalidArguments)?;
    let digest = pipeline_digest(&observation.raw_response);
    let row = pipeline_dispatch_row(tx, tenant, workspace, dispatch_id, true).await?;
    match row.state.as_str() {
        "sending" if row.send_certainty == "sent_unknown" => {
            let updated = sqlx::query(
                "UPDATE advisory_dispatch SET response_payload=$4,pipeline_response_sha256=$5,\
                 input_tokens=$6,output_tokens=$7,state='sealed',send_certainty='sent',\
                 outcome='provider_response',sealed_at=pg_catalog.clock_timestamp() \
                 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 \
                 AND state='sending' AND send_certainty='sent_unknown'",
            )
            .bind(tenant)
            .bind(workspace)
            .bind(dispatch_id)
            .bind(&observation.raw_response)
            .bind(&digest)
            .bind(input_tokens)
            .bind(output_tokens)
            .execute(&mut **tx)
            .await
            .map_err(storage_error)?;
            if updated.rows_affected() != 1 {
                return Err(Error::InputConflict);
            }
        }
        "sealed" => {
            if row.send_certainty != "sent"
                || row.outcome.as_deref() != Some("provider_response")
                || row.response_payload.as_deref() != Some(observation.raw_response.as_slice())
                || row.input_tokens != input_tokens
                || row.output_tokens != output_tokens
            {
                return Err(Error::InputConflict);
            }
        }
        _ => return Err(Error::InputConflict),
    }
    let sealed = pipeline_dispatch_row(tx, tenant, workspace, dispatch_id, false).await?;
    let saved_digest: Option<String> = sqlx::query_scalar(
        "SELECT pipeline_response_sha256 FROM advisory_dispatch \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(dispatch_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if saved_digest.as_deref() != Some(digest.as_str())
        || sealed.response_payload.as_deref() != Some(observation.raw_response.as_slice())
    {
        return Err(Error::InputConflict);
    }
    Ok(StoredPipelineRecommendationDispatch {
        dispatch: dispatch_from_row(&sealed)?,
        request_payload: sealed.request_payload,
        response_payload: sealed.response_payload.ok_or(Error::StorageUnavailable)?,
        response_sha256: digest,
    })
}

#[async_trait]
impl PipelineRecommendationDispatchStore for PgUnitOfWork {
    async fn authorize_pipeline_dispatch(
        &mut self,
        _capability: &PipelineDispatchCapability,
        workspace_id: Uuid,
        expected_config_revision: i64,
        authorization: &AdvisoryDispatchAuthorization,
    ) -> Result<AdvisoryDispatch> {
        let tenant = self.tenant_id()?;
        authorize_pipeline(
            self.transaction()?,
            tenant,
            workspace_id,
            expected_config_revision,
            authorization,
        )
        .await
    }

    async fn start_pipeline_dispatch(
        &mut self,
        _capability: &PipelineDispatchCapability,
        workspace_id: Uuid,
        dispatch_id: Uuid,
    ) -> Result<AdvisoryDispatchStart> {
        let tenant = self.tenant_id()?;
        start_pipeline(self.transaction()?, tenant, workspace_id, dispatch_id).await
    }

    async fn seal_pipeline_dispatch(
        &mut self,
        _capability: &PipelineDispatchCapability,
        workspace_id: Uuid,
        dispatch_id: Uuid,
        observation: &PipelineProviderObservation,
    ) -> Result<StoredPipelineRecommendationDispatch> {
        let tenant = self.tenant_id()?;
        seal_pipeline(
            self.transaction()?,
            tenant,
            workspace_id,
            dispatch_id,
            observation,
        )
        .await
    }
}
