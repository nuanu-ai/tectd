use super::*;

pub(super) async fn pipeline_recommendation_by_request(
    uow: &mut PgUnitOfWork,
    workspace_id: Uuid,
    request_key: &str,
) -> Result<Option<PreparedPipelineRecommendation>> {
    let Some(mut opportunity) = uow
        .advisory_opportunity_by_request(workspace_id, request_key)
        .await?
    else {
        return Ok(None);
    };
    if opportunity.capability != AdvisoryCapability::PipelineRecommendation {
        return Err(Error::InputConflict);
    }
    let tenant = uow.tenant_id()?;
    let row = sqlx::query(
        "SELECT o.scope_id AS scope_id,context.candidate_set_id,context.candidate_set_revision, \
                context.planning_snapshot_id,context.source_snapshot_id, \
                context.source_snapshot_digest,context.work_node_id,context.work_node_revision, \
                context.matrix_disposition_id,context.match_effect_attestation_id, \
                context.catalogue_revision,context.catalogue_digest,context.eligible_option_ids, \
                context.compatibility_policy_digest, \
                context.verification_contract_digest,context.manifest_payload, \
                context.manifest_digest,o.source_revision \
         FROM pipeline_advice_contexts context \
         JOIN advisory_opportunity o ON \
              (o.tenant_id,o.workspace_id,o.id)= \
              (context.tenant_id,context.workspace_id,context.opportunity_id) \
         WHERE context.tenant_id=$1 AND context.workspace_id=$2 \
           AND context.opportunity_id=$3",
    )
    .bind(tenant)
    .bind(workspace_id)
    .bind(opportunity.id)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?
    .ok_or(Error::InputConflict)?;
    let manifest: PipelineRecommendationManifest = serde_json::from_value(
        row.try_get::<Option<Value>, _>("manifest_payload")
            .map_err(storage_error)?
            .ok_or(Error::InputConflict)?,
    )
    .map_err(|_| Error::InputConflict)?;
    manifest.validate_digest()?;
    let source_snapshot_revision: String = row
        .try_get::<Option<String>, _>("source_revision")
        .map_err(storage_error)?
        .ok_or(Error::InputConflict)?;
    let context = PipelineRecommendationContext {
        scope_id: row.try_get("scope_id").map_err(storage_error)?,
        candidate_set_id: row.try_get("candidate_set_id").map_err(storage_error)?,
        candidate_set_revision: row
            .try_get("candidate_set_revision")
            .map_err(storage_error)?,
        planning_snapshot_id: row.try_get("planning_snapshot_id").map_err(storage_error)?,
        source_snapshot_id: row.try_get("source_snapshot_id").map_err(storage_error)?,
        source_snapshot_revision,
        source_snapshot_digest: row
            .try_get("source_snapshot_digest")
            .map_err(storage_error)?,
        work_node_id: row.try_get("work_node_id").map_err(storage_error)?,
        work_node_revision: row.try_get("work_node_revision").map_err(storage_error)?,
        matrix_disposition_id: row
            .try_get("matrix_disposition_id")
            .map_err(storage_error)?,
        match_effect_attestation_id: row
            .try_get("match_effect_attestation_id")
            .map_err(storage_error)?,
        catalogue_revision: row.try_get("catalogue_revision").map_err(storage_error)?,
        catalogue_digest: row.try_get("catalogue_digest").map_err(storage_error)?,
        compatibility_policy_digest: row
            .try_get("compatibility_policy_digest")
            .map_err(storage_error)?,
        eligible_option_ids: row.try_get("eligible_option_ids").map_err(storage_error)?,
        verification_contract_digest: row
            .try_get("verification_contract_digest")
            .map_err(storage_error)?,
    };
    let stored_digest: Option<String> = row.try_get("manifest_digest").map_err(storage_error)?;
    if stored_digest.as_deref() != Some(manifest.digest.as_str())
        || context.verification_contract_digest != manifest.digest
        || context.compatibility_policy_digest != manifest.compatibility_policy_digest
        || opportunity.material_digest != manifest.digest
    {
        return Err(Error::InputConflict);
    }
    opportunity.work_revision = Some(context.work_node_revision);
    opportunity.source_ref = Some(context.source_snapshot_id.to_string());
    Ok(Some(PreparedPipelineRecommendation {
        opportunity,
        context,
        manifest,
    }))
}

pub(super) async fn capture_pipeline_recommendation(
    uow: &mut PgUnitOfWork,
    workspace_id: Uuid,
    input: &AdvisoryOpportunityInput,
    context: &PipelineRecommendationContext,
    manifest: &PipelineRecommendationManifest,
) -> Result<PreparedPipelineRecommendation> {
    input.validate()?;
    manifest.validate_digest()?;
    let actor = uow.principal_id()?;
    if input.capability != AdvisoryCapability::PipelineRecommendation
        || input.decision_point != AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen
        || input.authorized_actor_id != actor
        || input.target_id != Some(context.work_node_id)
        || input.work_revision != Some(context.work_node_revision)
        || input.material_digest != manifest.digest
        || context.verification_contract_digest != manifest.digest
        || manifest.work_id != context.work_node_id
        || manifest.work_revision != context.work_node_revision
        || manifest.catalogue_revision != context.catalogue_revision
        || manifest.catalogue_digest != context.catalogue_digest
        || manifest.compatibility_policy_digest != context.compatibility_policy_digest
        || context.eligible_option_ids
            != manifest
                .options
                .iter()
                .map(|o| o.id.clone())
                .collect::<Vec<_>>()
    {
        return Err(Error::InputConflict);
    }
    if let Some(prior) = uow
        .pipeline_recommendation_by_request(workspace_id, &input.workflow_occurrence_key)
        .await?
    {
        return if prior.context == *context
            && prior.manifest == *manifest
            && prior.opportunity.session_id == input.session_id
            && prior.opportunity.authorized_actor_id == input.authorized_actor_id
            && prior.opportunity.session_preference == input.session_preference
            && prior.opportunity.request_preference == input.request_preference
            && prior.opportunity.config_revision == input.config_revision
            && prior.opportunity.state == input.state
            && prior.opportunity.primary_reason == input.primary_reason
        {
            Ok(prior)
        } else {
            Err(Error::InputConflict)
        };
    }
    let current = uow
        .load_pipeline_recommendation_basis(
            workspace_id,
            context.candidate_set_id,
            context.work_node_id,
            true,
        )
        .await?
        .ok_or(Error::StaleContext)?;
    if current.scope_id != context.scope_id
        || current.candidate_set_revision != context.candidate_set_revision
        || current.planning_snapshot_id != context.planning_snapshot_id
        || current.source_snapshot_id != context.source_snapshot_id
        || current.source_candidate_set_revision.to_string() != context.source_snapshot_revision
        || pipeline_recommendation_source_digest(
            current.source_snapshot_id,
            current.source_candidate_set_revision,
            &current.selected_sources_digest,
        )? != context.source_snapshot_digest
        || current.source.work.revision() != context.work_node_revision
        || current.matrix_disposition_id != context.matrix_disposition_id
        || current.match_effect_attestation_id != context.match_effect_attestation_id
        || current.source.catalogue.revision != context.catalogue_revision
        || current.source.catalogue.digest != context.catalogue_digest
        || context.compatibility_policy_digest != manifest.compatibility_policy_digest
        || manifest.matrix_input_digest != matrix_input_digest(&current.source.matrix.input)?
        || manifest.selected_candidate_digest != selected_candidate_digest(&current.source)?
        || manifest.matrix_task_id != current.source.matrix.composition.task_id
        || manifest.matrix_task_revision != current.source.matrix.composition.task_revision
        || manifest.selected_choice_id != current.source.matrix.selected_choice_id
        || manifest.matrix_choice_set_digest != current.source.matrix.choice_set_digest
        || manifest.matrix_verification_digest != current.source.matrix.verification_digest
        || manifest.mandatory_card_ids != current.source.matrix.saved_mandatory_card_ids
    {
        return Err(Error::StaleContext);
    }
    let tenant = uow.tenant_id()?;
    let authorized: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM agent_sessions s \
         JOIN memberships m ON (m.tenant_id,m.workspace_id,m.principal_id)= \
              (s.tenant_id,s.workspace_id,$4) \
         WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.id=$3 \
           AND public.tect_dk_session_principal(s.id)=$4 \
           AND public.tect_dk_is_owner($4))",
    )
    .bind(tenant)
    .bind(workspace_id)
    .bind(input.session_id)
    .bind(actor)
    .fetch_one(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    if !authorized {
        return Err(Error::Forbidden);
    }
    sqlx::query(
        "INSERT INTO advisory_workspace_config_history \
             (tenant_id,workspace_id,revision,previous_revision,mode, \
              provider_profile_ref,model_configuration, \
              changed_by_principal_id,changed_by_session_id) \
         VALUES ($1,$2,0,NULL,'disabled',NULL,NULL,$3,$4) ON CONFLICT DO NOTHING",
    )
    .bind(tenant)
    .bind(workspace_id)
    .bind(actor)
    .bind(input.session_id)
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(write_error)?;
    sqlx::query(
        "INSERT INTO advisory_workspace_config \
             (tenant_id,workspace_id,revision,mode,provider_profile_ref,model_configuration, \
              updated_by_principal_id,updated_by_session_id) \
         VALUES ($1,$2,0,'disabled',NULL,NULL,$3,$4) ON CONFLICT DO NOTHING",
    )
    .bind(tenant)
    .bind(workspace_id)
    .bind(actor)
    .bind(input.session_id)
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(write_error)?;
    let config: Option<(i64, String, Option<String>, Option<Value>)> = sqlx::query_as(
        "SELECT revision,mode,provider_profile_ref,model_configuration \
         FROM advisory_workspace_config \
         WHERE tenant_id=$1 AND workspace_id=$2 FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace_id)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    let (config_revision, mode, provider_profile_ref, model_configuration) =
        config.ok_or(Error::InternalInvariant)?;
    if config_revision != input.config_revision {
        return Err(Error::StaleRevision);
    }
    let required_no_call = if mode == "disabled" {
        Some(AdvisoryReason::WorkspaceDisabled)
    } else if input.session_preference == AdvisoryRequestPreference::Skip {
        Some(AdvisoryReason::SessionSkip)
    } else if input.request_preference == AdvisoryRequestPreference::Skip {
        Some(AdvisoryReason::RequestSkip)
    } else if input.primary_reason == AdvisoryReason::CapabilityUnavailable
        && input.state == AdvisoryOpportunityState::NoCall
    {
        Some(AdvisoryReason::CapabilityUnavailable)
    } else if manifest.options.len() < 2 {
        Some(AdvisoryReason::ChoiceSetNotApplicable)
    } else if provider_profile_ref.is_none() || model_configuration.is_none() {
        Some(AdvisoryReason::ProviderUnconfigured)
    } else {
        None
    };
    if required_no_call.is_some_and(|reason| {
        input.state != AdvisoryOpportunityState::NoCall || input.primary_reason != reason
    }) || required_no_call.is_none()
        && (input.state != AdvisoryOpportunityState::Prepared
            || input.primary_reason != AdvisoryReason::RecommendationPrepared)
    {
        return Err(Error::InputConflict);
    }
    let opportunity_id = Uuid::new_v4();
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO advisory_opportunity \
             (id,tenant_id,workspace_id,scope_id,work_item_kind,work_item_id, \
              session_id,authorized_actor_id,source_revision,capability,decision_point, \
              config_revision,session_preference,request_preference,policy_version, \
              request_key,material_digest,state,primary_reason) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19) \
         ON CONFLICT(tenant_id,workspace_id,request_key) DO NOTHING RETURNING id",
    )
    .bind(opportunity_id)
    .bind(tenant)
    .bind(workspace_id)
    .bind(context.scope_id)
    .bind(&input.target_kind)
    .bind(input.target_id)
    .bind(input.session_id)
    .bind(input.authorized_actor_id)
    .bind(&context.source_snapshot_revision)
    .bind(input.capability.as_str())
    .bind(input.decision_point.as_str())
    .bind(input.config_revision)
    .bind(input.session_preference.as_str())
    .bind(input.request_preference.as_str())
    .bind(ADVISORY_POLICY_VERSION)
    .bind(&input.workflow_occurrence_key)
    .bind(&input.material_digest)
    .bind(input.state.as_str())
    .bind(input.primary_reason.as_str())
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(write_error)?;
    if inserted.is_none() {
        let prior = uow
            .pipeline_recommendation_by_request(workspace_id, &input.workflow_occurrence_key)
            .await?
            .ok_or(Error::InputConflict)?;
        return if prior.context == *context
            && prior.manifest == *manifest
            && prior.opportunity.session_id == input.session_id
            && prior.opportunity.authorized_actor_id == input.authorized_actor_id
        {
            Ok(prior)
        } else {
            Err(Error::InputConflict)
        };
    }
    sqlx::query(
        "INSERT INTO pipeline_advice_contexts \
             (tenant_id,workspace_id,opportunity_id,candidate_set_id, \
              candidate_set_revision,planning_snapshot_id,source_snapshot_id, \
              work_node_id,work_node_revision,source_snapshot_digest, \
              matrix_disposition_id,match_effect_attestation_id, \
              catalogue_revision,catalogue_digest,eligible_option_ids, \
              compatibility_policy_digest, \
              verification_contract_digest,manifest_payload,manifest_digest) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)",
    )
    .bind(tenant)
    .bind(workspace_id)
    .bind(opportunity_id)
    .bind(context.candidate_set_id)
    .bind(context.candidate_set_revision)
    .bind(context.planning_snapshot_id)
    .bind(context.source_snapshot_id)
    .bind(context.work_node_id)
    .bind(context.work_node_revision)
    .bind(&context.source_snapshot_digest)
    .bind(context.matrix_disposition_id)
    .bind(context.match_effect_attestation_id)
    .bind(&context.catalogue_revision)
    .bind(&context.catalogue_digest)
    .bind(&context.eligible_option_ids)
    .bind(&context.compatibility_policy_digest)
    .bind(&context.verification_contract_digest)
    .bind(serde_json::to_value(manifest).map_err(storage_error)?)
    .bind(&manifest.digest)
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(write_error)?;
    uow.pipeline_recommendation_by_request(workspace_id, &input.workflow_occurrence_key)
        .await?
        .ok_or(Error::InternalInvariant)
}
