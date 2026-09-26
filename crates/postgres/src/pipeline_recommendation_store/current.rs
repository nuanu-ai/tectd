use super::*;

pub(super) async fn pipeline_recommendation_by_opportunity(
    uow: &mut PgUnitOfWork,
    workspace_id: Uuid,
    opportunity_id: Uuid,
) -> Result<Option<PreparedPipelineRecommendation>> {
    let tenant = uow.tenant_id()?;
    let request_key: Option<String> = sqlx::query_scalar(
        "SELECT request_key FROM advisory_opportunity \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 \
           AND capability='pipeline_recommendation'",
    )
    .bind(tenant)
    .bind(workspace_id)
    .bind(opportunity_id)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    let Some(request_key) = request_key else {
        return Ok(None);
    };
    let loaded = uow
        .pipeline_recommendation_by_request(workspace_id, &request_key)
        .await?;
    if loaded
        .as_ref()
        .is_some_and(|saved| saved.opportunity.id != opportunity_id)
    {
        return Err(Error::InputConflict);
    }
    Ok(loaded)
}

pub(super) async fn pipeline_recommendation_is_current(
    uow: &mut PgUnitOfWork,
    workspace_id: Uuid,
    saved: &PreparedPipelineRecommendation,
) -> Result<bool> {
    // The original receipt is a lookup key, not authority. The opportunity
    // may advance to awaiting_response after the send, so compare its
    // immutable capture fields while allowing that lifecycle transition.
    let persisted = uow
        .pipeline_recommendation_by_opportunity(workspace_id, saved.opportunity.id)
        .await?;
    let Some(persisted) = persisted else {
        return Ok(false);
    };
    let old = &saved.opportunity;
    let now = &persisted.opportunity;
    if saved.context != persisted.context
        || saved.manifest != persisted.manifest
        || old.workspace_id != workspace_id
        || !matches!(
            old.state,
            AdvisoryOpportunityState::Prepared
                | AdvisoryOpportunityState::AwaitingResponse
                | AdvisoryOpportunityState::Advised
        )
        || !matches!(
            now.state,
            AdvisoryOpportunityState::Prepared
                | AdvisoryOpportunityState::AwaitingResponse
                | AdvisoryOpportunityState::Advised
        )
        || old.id != now.id
        || old.workspace_id != now.workspace_id
        || old.session_id != now.session_id
        || old.authorized_actor_id != now.authorized_actor_id
        || old.capability != now.capability
        || old.decision_point != now.decision_point
        || old.decision_point_version != now.decision_point_version
        || old.workflow_occurrence_key != now.workflow_occurrence_key
        || old.target_kind != now.target_kind
        || old.target_id != now.target_id
        || old.work_revision != now.work_revision
        || old.matrix_task_revision != now.matrix_task_revision
        || old.matrix_choice_set_digest != now.matrix_choice_set_digest
        || old.matrix_verification_digest != now.matrix_verification_digest
        || old.source_ref != now.source_ref
        || old.session_preference != now.session_preference
        || old.request_preference != now.request_preference
        || old.config_revision != now.config_revision
        || old.material_digest != now.material_digest
        || now.capability != AdvisoryCapability::PipelineRecommendation
        || now.decision_point != AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen
        || saved.manifest.validate_digest().is_err()
        || old.material_digest != saved.manifest.digest
        || saved.context.verification_contract_digest != saved.manifest.digest
        || saved.context.work_node_id != saved.manifest.work_id
        || saved.context.work_node_revision != saved.manifest.work_revision
        || saved.context.catalogue_revision != saved.manifest.catalogue_revision
        || saved.context.catalogue_digest != saved.manifest.catalogue_digest
        || saved.context.compatibility_policy_digest != saved.manifest.compatibility_policy_digest
        || saved.context.eligible_option_ids
            != saved
                .manifest
                .options
                .iter()
                .map(|option| option.id.clone())
                .collect::<Vec<_>>()
    {
        return Ok(false);
    }

    let tenant = uow.tenant_id()?;
    let mut config_query = String::from(
        "SELECT revision,mode,provider_profile_ref IS NOT NULL AS provider_configured, \
                model_configuration IS NOT NULL AS model_configured \
         FROM advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2",
    );
    let for_update = uow.is_read_write();
    if for_update {
        config_query.push_str(" FOR SHARE");
    }
    let config: Option<(i64, String, bool, bool)> = sqlx::query_as(&config_query)
        .bind(tenant)
        .bind(workspace_id)
        .fetch_optional(&mut **uow.transaction()?)
        .await
        .map_err(storage_error)?;
    if !config.is_some_and(|(revision, mode, provider, model)| {
        revision == old.config_revision && mode == "optional" && provider && model
    }) || !saved.manifest.should_call()
    {
        return Ok(false);
    }

    // The writer locks the mutable heads through provider-send admission;
    // the post-seal read-only transaction has a repeatable-read snapshot.
    // The DB dispatch guard independently rechecks the same path at INSERT.
    let basis = match uow
        .load_pipeline_recommendation_basis(
            workspace_id,
            saved.context.candidate_set_id,
            saved.context.work_node_id,
            for_update,
        )
        .await
    {
        Ok(Some(basis)) => basis,
        Ok(None) | Err(Error::StaleContext) => return Ok(false),
        Err(error) => return Err(error),
    };
    let context = &saved.context;
    let manifest = &saved.manifest;
    let source = &basis.source;
    Ok(basis.scope_id == context.scope_id
        && basis.candidate_set_id == context.candidate_set_id
        && basis.candidate_set_revision == context.candidate_set_revision
        && basis.planning_snapshot_id == context.planning_snapshot_id
        && basis.source_snapshot_id == context.source_snapshot_id
        && basis.source_candidate_set_revision.to_string() == context.source_snapshot_revision
        && pipeline_recommendation_source_digest(
            basis.source_snapshot_id,
            basis.source_candidate_set_revision,
            &basis.selected_sources_digest,
        )? == context.source_snapshot_digest
        && source.work.revision() == context.work_node_revision
        && basis.matrix_disposition_id == context.matrix_disposition_id
        && basis.match_effect_attestation_id == context.match_effect_attestation_id
        && source.catalogue.revision == context.catalogue_revision
        && source.catalogue.digest == context.catalogue_digest
        && context.compatibility_policy_digest == manifest.compatibility_policy_digest
        && manifest.matrix_input_digest == matrix_input_digest(&source.matrix.input)?
        && manifest.selected_candidate_digest == selected_candidate_digest(source)?
        && manifest.matrix_task_id == source.matrix.composition.task_id
        && manifest.matrix_task_revision == source.matrix.composition.task_revision
        && manifest.selected_choice_id == source.matrix.selected_choice_id
        && manifest.matrix_choice_set_digest == source.matrix.choice_set_digest
        && manifest.matrix_verification_digest == source.matrix.verification_digest
        && manifest.mandatory_card_ids == source.matrix.saved_mandatory_card_ids)
}
