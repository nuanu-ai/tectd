//! Prepare one pre-open recommendation from a current saved Work node.
//! No provider, pipeline, phase, or verifier operation is reachable here.

use crate::{
    PipelineRecommendationBasis, PipelineRecommendationContext, PreparedPipelineRecommendation,
    TransactionMode, WorkspaceService,
};
use sha2::{Digest, Sha256};
use tect_domain::{
    ADVISORY_DECISION_POINT_VERSION, AdvisoryCapability, AdvisoryDecisionPoint,
    AdvisoryOpportunityInput, AdvisoryOpportunityState, AdvisoryReason, AdvisoryRequestPreference,
    Error, PipelineKind, RequestContext, Result, SliceCandidateNode, WorkspaceAdvisoryMode,
    build_pipeline_recommendation_manifest,
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparePipelineRecommendation {
    pub candidate_set_id: Uuid,
    pub expected_candidate_set_revision: i64,
    pub work_node_id: Uuid,
    pub expected_work_node_revision: i64,
    pub request_key: String,
    pub session_preference: AdvisoryRequestPreference,
    pub request_preference: AdvisoryRequestPreference,
}

impl PreparePipelineRecommendation {
    fn validate(&self) -> Result<()> {
        if self.candidate_set_id.is_nil()
            || self.work_node_id.is_nil()
            || self.expected_candidate_set_revision < 2
            || self.expected_work_node_revision < 1
            || self.request_key.is_empty()
            || self.request_key.len() > 256
            || self.request_key.contains('\0')
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

impl WorkspaceService {
    pub async fn prepare_pipeline_recommendation(
        &self,
        context: &RequestContext,
        request: &PreparePipelineRecommendation,
    ) -> Result<PreparedPipelineRecommendation> {
        request.validate()?;
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let store = tx.pipeline_recommendation_store().ok_or(Error::Forbidden)?;
        if let Some(saved) = store
            .pipeline_recommendation_by_request(workspace.id, &request.request_key)
            .await?
        {
            if !replay_matches(
                &saved,
                request,
                workspace.id,
                session.id,
                identity.principal_id,
            )? {
                return Err(Error::InputConflict);
            }
            tx.commit().await?;
            return Ok(saved);
        }
        let basis = store
            .load_pipeline_recommendation_basis(
                workspace.id,
                request.candidate_set_id,
                request.work_node_id,
                true,
            )
            .await?
            .ok_or(Error::NotFound)?;
        validate_basis(&basis, request)?;
        let mut source = basis.source.clone();
        source.definitions.clear();
        for kind in PipelineKind::CURRENT_SLICE_RUN_KINDS {
            if let Some(definition) = self
                .pipeline_recommendation_definitions
                .definition(&source.catalogue.revision, kind)?
            {
                source.definitions.push(definition);
            }
        }
        let manifest = build_pipeline_recommendation_manifest(&source)?;
        let config = tx.advisory_config(workspace.id).await?;
        let (state, primary_reason) = preparation_decision(
            config.mode,
            request,
            manifest.should_call(),
            self.pipeline_recommendation_definitions.available()
                && self.pipeline_recommendation_provider.available(),
            config.provider_configured(),
        );
        let input = AdvisoryOpportunityInput {
            session_id: session.id,
            authorized_actor_id: identity.principal_id,
            capability: AdvisoryCapability::PipelineRecommendation,
            decision_point: AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen,
            decision_point_version: ADVISORY_DECISION_POINT_VERSION,
            workflow_occurrence_key: request.request_key.clone(),
            target_kind: "slice_candidate_node".into(),
            target_id: Some(request.work_node_id),
            work_revision: Some(request.expected_work_node_revision),
            matrix_task_revision: None,
            matrix_choice_set_digest: None,
            matrix_verification_digest: None,
            source_ref: Some(basis.source_snapshot_id.to_string()),
            parent_opportunity_id: None,
            session_preference: request.session_preference,
            request_preference: request.request_preference,
            config_revision: config.revision,
            material_digest: manifest.digest.clone(),
            state,
            primary_reason,
        };
        input.validate()?;
        let stored_context = PipelineRecommendationContext::from_basis(&basis, &manifest);
        // The adapter repeats currentness and config checks while holding
        // locks, then inserts all three records atomically in this UoW.
        let saved = tx
            .pipeline_recommendation_store()
            .ok_or(Error::Forbidden)?
            .capture_pipeline_recommendation(workspace.id, &input, &stored_context, &manifest)
            .await?;
        verify_capture(&saved, &input, &stored_context, &manifest)?;
        tx.commit().await?;
        Ok(saved)
    }
}

impl PipelineRecommendationContext {
    fn from_basis(
        basis: &PipelineRecommendationBasis,
        manifest: &tect_domain::PipelineRecommendationManifest,
    ) -> Self {
        Self {
            scope_id: basis.scope_id,
            candidate_set_id: basis.candidate_set_id,
            candidate_set_revision: basis.candidate_set_revision,
            planning_snapshot_id: basis.planning_snapshot_id,
            source_snapshot_id: basis.source_snapshot_id,
            source_snapshot_revision: basis.source_candidate_set_revision.to_string(),
            source_snapshot_digest: pipeline_recommendation_source_digest(
                basis.source_snapshot_id,
                basis.source_candidate_set_revision,
                &basis.selected_sources_digest,
            )
            .expect("validated basis"),
            work_node_id: manifest.work_id,
            work_node_revision: manifest.work_revision,
            matrix_disposition_id: basis.matrix_disposition_id,
            match_effect_attestation_id: basis.match_effect_attestation_id,
            catalogue_revision: manifest.catalogue_revision.clone(),
            catalogue_digest: manifest.catalogue_digest.clone(),
            eligible_kind_ids: manifest
                .options
                .iter()
                .map(|option| option.id.clone())
                .collect(),
            verification_contract_digest: manifest.digest.clone(),
        }
    }
}

fn validate_basis(
    basis: &PipelineRecommendationBasis,
    request: &PreparePipelineRecommendation,
) -> Result<()> {
    if basis.candidate_set_id != request.candidate_set_id
        || basis.candidate_set_revision != request.expected_candidate_set_revision
        || basis.source.work.id() != request.work_node_id
        || basis.source.work.revision() != request.expected_work_node_revision
        || !matches!(basis.source.work, SliceCandidateNode::Work { .. })
        || basis.scope_id.is_nil()
        || basis.planning_snapshot_id.is_nil()
        || basis.source_snapshot_id.is_nil()
        || basis.matrix_disposition_id.is_nil()
        || basis.match_effect_attestation_id.is_nil()
        || basis.source_candidate_set_revision < 1
        || !valid_digest(&basis.selected_sources_digest)
        || !valid_digest(&basis.source.catalogue.digest)
    {
        return Err(Error::StaleContext);
    }
    Ok(())
}

fn preparation_decision(
    mode: WorkspaceAdvisoryMode,
    request: &PreparePipelineRecommendation,
    has_options: bool,
    capability_available: bool,
    provider_configured: bool,
) -> (AdvisoryOpportunityState, AdvisoryReason) {
    let no_call = if mode == WorkspaceAdvisoryMode::Disabled {
        Some(AdvisoryReason::WorkspaceDisabled)
    } else if request.session_preference == AdvisoryRequestPreference::Skip {
        Some(AdvisoryReason::SessionSkip)
    } else if request.request_preference == AdvisoryRequestPreference::Skip {
        Some(AdvisoryReason::RequestSkip)
    } else if !capability_available {
        Some(AdvisoryReason::CapabilityUnavailable)
    } else if !has_options {
        Some(AdvisoryReason::ChoiceSetNotApplicable)
    } else if !provider_configured {
        Some(AdvisoryReason::ProviderUnconfigured)
    } else {
        None
    };
    match no_call {
        Some(reason) => (AdvisoryOpportunityState::NoCall, reason),
        None => (
            AdvisoryOpportunityState::Prepared,
            AdvisoryReason::RecommendationPrepared,
        ),
    }
}

fn replay_matches(
    saved: &PreparedPipelineRecommendation,
    request: &PreparePipelineRecommendation,
    workspace_id: Uuid,
    session_id: Uuid,
    actor_id: Uuid,
) -> Result<bool> {
    saved.manifest.validate_digest()?;
    Ok(saved.opportunity.workspace_id == workspace_id
        && saved.opportunity.session_id == session_id
        && saved.opportunity.authorized_actor_id == actor_id
        && saved.opportunity.capability == AdvisoryCapability::PipelineRecommendation
        && saved.opportunity.decision_point
            == AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen
        && saved.opportunity.workflow_occurrence_key == request.request_key
        && saved.opportunity.target_id == Some(request.work_node_id)
        && saved.opportunity.work_revision == Some(request.expected_work_node_revision)
        && saved.opportunity.session_preference == request.session_preference
        && saved.opportunity.request_preference == request.request_preference
        && saved.opportunity.material_digest == saved.manifest.digest
        && saved.context.candidate_set_id == request.candidate_set_id
        && saved.context.candidate_set_revision == request.expected_candidate_set_revision
        && saved.context.work_node_id == request.work_node_id
        && saved.context.work_node_revision == request.expected_work_node_revision
        && saved.context.verification_contract_digest == saved.manifest.digest)
}

fn verify_capture(
    saved: &PreparedPipelineRecommendation,
    input: &AdvisoryOpportunityInput,
    context: &PipelineRecommendationContext,
    manifest: &tect_domain::PipelineRecommendationManifest,
) -> Result<()> {
    if saved.context != *context
        || saved.manifest != *manifest
        || saved.opportunity.workflow_occurrence_key != input.workflow_occurrence_key
        || saved.opportunity.material_digest != input.material_digest
        || saved.opportunity.state != input.state
        || saved.opportunity.primary_reason != input.primary_reason
    {
        return Err(Error::InternalInvariant);
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// A versioned binding to the persisted source snapshot identity, its source
/// set revision in the planning snapshot, and its selected-source digest.
pub fn pipeline_recommendation_source_digest(
    source_snapshot_id: Uuid,
    source_candidate_set_revision: i64,
    selected_sources_digest: &str,
) -> Result<String> {
    if source_snapshot_id.is_nil()
        || source_candidate_set_revision < 1
        || !valid_digest(selected_sources_digest)
    {
        return Err(Error::InvalidArguments);
    }
    let mut hash = Sha256::new();
    hash.update(b"tect.pipeline-recommendation-source-binding/1\0");
    hash.update(source_snapshot_id.as_bytes());
    hash.update(source_candidate_set_revision.to_be_bytes());
    hash.update(selected_sources_digest.as_bytes());
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_call_reasons_precede_dispatch() {
        let request = PreparePipelineRecommendation {
            candidate_set_id: Uuid::new_v4(),
            expected_candidate_set_revision: 2,
            work_node_id: Uuid::new_v4(),
            expected_work_node_revision: 1,
            request_key: "one".into(),
            session_preference: AdvisoryRequestPreference::UseWorkspace,
            request_preference: AdvisoryRequestPreference::UseWorkspace,
        };
        assert_eq!(
            preparation_decision(WorkspaceAdvisoryMode::Optional, &request, false, true, true),
            (
                AdvisoryOpportunityState::NoCall,
                AdvisoryReason::ChoiceSetNotApplicable
            )
        );
        assert_eq!(
            preparation_decision(WorkspaceAdvisoryMode::Optional, &request, true, true, true),
            (
                AdvisoryOpportunityState::Prepared,
                AdvisoryReason::RecommendationPrepared
            )
        );
        assert_eq!(
            preparation_decision(WorkspaceAdvisoryMode::Disabled, &request, true, true, true),
            (
                AdvisoryOpportunityState::NoCall,
                AdvisoryReason::WorkspaceDisabled
            )
        );
    }
}
