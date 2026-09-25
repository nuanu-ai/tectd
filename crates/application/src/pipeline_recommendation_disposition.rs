//! Record one planning disposition from exact saved recommendation material.

use crate::{PipelineRecommendationStore, TransactionMode, WorkspaceService};
use tect_domain::{
    AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryOpportunityState, Error,
    PipelineDispositionAdvice, PipelineDispositionRequest, PipelineDispositionResult,
    RequestContext, Result,
};
use uuid::Uuid;

impl WorkspaceService {
    pub async fn dispose_pipeline_recommendation(
        &self,
        context: &RequestContext,
        request: &PipelineDispositionRequest,
    ) -> Result<PipelineDispositionResult> {
        request.validate()?;
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let current_policy_digest = self.current_pipeline_policy()?.digest()?;
        let store = tx.pipeline_recommendation_store().ok_or(Error::Forbidden)?;
        let prepared = store
            .pipeline_recommendation_by_opportunity(workspace.id, request.opportunity_id)
            .await?
            .ok_or(Error::NotFound)?;
        self.validate_pipeline_recommendation_definitions(&prepared.manifest)?;
        let result = dispose_in_store(
            store,
            workspace.id,
            session.id,
            identity.principal_id,
            request,
            &current_policy_digest,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }
}

async fn dispose_in_store(
    store: &mut dyn PipelineRecommendationStore,
    workspace_id: Uuid,
    session_id: Uuid,
    actor_id: Uuid,
    request: &PipelineDispositionRequest,
    current_policy_digest: &str,
) -> Result<PipelineDispositionResult> {
    let prepared = store
        .pipeline_recommendation_by_opportunity(workspace_id, request.opportunity_id)
        .await?
        .ok_or(Error::NotFound)?;
    let opportunity = &prepared.opportunity;
    if opportunity.id != request.opportunity_id
        || opportunity.workspace_id != workspace_id
        || opportunity.session_id != session_id
        || opportunity.authorized_actor_id != actor_id
    {
        return Err(Error::Forbidden);
    }
    if current_policy_digest != prepared.context.compatibility_policy_digest
        || prepared.context.compatibility_policy_digest
            != prepared.manifest.compatibility_policy_digest
    {
        return Err(Error::StaleContext);
    }
    if let Some(saved) = store
        .pipeline_disposition_by_opportunity(workspace_id, request.opportunity_id)
        .await?
    {
        if saved.request != *request {
            return Err(Error::InputConflict);
        }
        return Ok(saved);
    }
    let basis = store
        .load_pipeline_disposition_basis(workspace_id, request.opportunity_id)
        .await?
        .ok_or(Error::NotFound)?;
    if basis.prepared != prepared {
        return Err(Error::InputConflict);
    }
    let opportunity = &prepared.opportunity;
    if opportunity.id != request.opportunity_id
        || opportunity.workspace_id != workspace_id
        || opportunity.session_id != session_id
        || opportunity.authorized_actor_id != actor_id
        || opportunity.capability != AdvisoryCapability::PipelineRecommendation
        || opportunity.decision_point
            != AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen
        || opportunity.material_digest != prepared.manifest.digest
        || opportunity.target_id != Some(prepared.context.work_node_id)
        || opportunity.work_revision != Some(prepared.context.work_node_revision)
        || prepared.context.verification_contract_digest != prepared.manifest.digest
        || prepared.context.eligible_option_ids
            != prepared
                .manifest
                .options
                .iter()
                .map(|option| option.id.clone())
                .collect::<Vec<_>>()
    {
        return Err(Error::InputConflict);
    }
    match (&basis.advice, opportunity.state) {
        (PipelineDispositionAdvice::NoCall, AdvisoryOpportunityState::NoCall) => {}
        (
            PipelineDispositionAdvice::Ranked { .. } | PipelineDispositionAdvice::Abstained { .. },
            AdvisoryOpportunityState::AwaitingResponse | AdvisoryOpportunityState::Advised,
        ) => {}
        _ => return Err(Error::InputConflict),
    }
    if !store
        .pipeline_disposition_is_current(workspace_id, &basis)
        .await?
    {
        return Err(Error::StaleContext);
    }
    let result = request.resolve(
        uuid::Uuid::new_v4(),
        &prepared.manifest,
        &basis.saved_work,
        &basis.advice,
    )?;
    let saved = store
        .capture_pipeline_disposition(workspace_id, &result)
        .await?;
    // A concurrent identical capture can return the first transaction's
    // immutable receipt, whose generated ID differs from this attempt's ID.
    if saved.request != result.request
        || saved.work_id != result.work_id
        || saved.advice != result.advice
        || saved.selected_kind != result.selected_kind
        || saved.selected_option_id != result.selected_option_id
    {
        return Err(Error::InputConflict);
    }
    Ok(saved)
}

#[cfg(test)]
#[path = "pipeline_recommendation_disposition_tests.rs"]
mod tests;
