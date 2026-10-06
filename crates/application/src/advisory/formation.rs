use super::*;

pub(crate) fn scope_decomposition_opportunity(
    session_id: uuid::Uuid,
    authorized_actor_id: uuid::Uuid,
    request: &tect_domain::BeginCandidateSet,
    config: &WorkspaceAdvisoryConfig,
    session_preference: tect_domain::AdvisoryRequestPreference,
    deterministic_input_valid: bool,
) -> AdvisoryOpportunityInput {
    let canonical_material = serde_json::to_vec(&(
        request,
        config.revision,
        config.mode.as_str(),
        session_preference.as_str(),
        deterministic_input_valid,
        tect_domain::ADVISORY_POLICY_VERSION,
    ))
    .expect("advisory material contains only serializable domain values");
    let material_digest = format!("{:x}", Sha256::digest(&canonical_material));
    let decision = tect_domain::assess_advisory_policy(tect_domain::AdvisoryPolicyInput {
        workspace_mode: config.mode,
        session_preference,
        request_preference: request.advisory_preference,
        deterministic_input_valid,
        capability_available: false,
        provider_configured: config.provider_configured(),
    });
    AdvisoryOpportunityInput {
        session_id,
        authorized_actor_id,
        capability: AdvisoryCapability::ScopeDecomposition,
        decision_point: AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection,
        decision_point_version: ADVISORY_DECISION_POINT_VERSION,
        workflow_occurrence_key: request.request_id.to_string(),
        target_kind: "program".into(),
        target_id: Some(request.program_id),
        work_revision: Some(request.program_revision),
        matrix_task_revision: None,
        matrix_choice_set_digest: None,
        matrix_verification_digest: None,
        source_ref: None,
        parent_opportunity_id: None,
        session_preference,
        request_preference: request.advisory_preference,
        config_revision: config.revision,
        material_digest,
        state: decision.state,
        primary_reason: decision.reason,
    }
}

#[async_trait]
pub(crate) trait DurableOpportunityBoundary: Send {
    async fn capture(
        &mut self,
        workspace_id: uuid::Uuid,
        input: &AdvisoryOpportunityInput,
    ) -> Result<tect_domain::AdvisoryOpportunity>;
    async fn commit_boundary(self) -> Result<()>;
}

#[async_trait]
impl DurableOpportunityBoundary for Box<dyn UnitOfWork> {
    async fn capture(
        &mut self,
        workspace_id: uuid::Uuid,
        input: &AdvisoryOpportunityInput,
    ) -> Result<tect_domain::AdvisoryOpportunity> {
        self.capture_advisory_opportunity(workspace_id, input).await
    }

    async fn commit_boundary(self) -> Result<()> {
        self.commit().await
    }
}

pub(crate) async fn commit_scope_opportunity<B: DurableOpportunityBoundary>(
    mut boundary: B,
    workspace_id: uuid::Uuid,
    input: &AdvisoryOpportunityInput,
) -> Result<tect_domain::AdvisoryOpportunity> {
    let opportunity = boundary.capture(workspace_id, input).await?;
    boundary.commit_boundary().await?;
    Ok(opportunity)
}
