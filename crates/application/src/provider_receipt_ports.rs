use sha2::{Digest, Sha256};
use tect_domain::{
    AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryDispatch, AdvisoryDispatchAuthorization,
    AdvisoryDispatchStart, AdvisoryDispatchState, AdvisoryOpportunity, AdvisorySendCertainty,
    Error, Result,
};
use uuid::Uuid;

/// Application-held continuation for an already committed provider send.
/// It grants receipt retention and accounting, never parsing or user effects.
#[derive(Debug, Clone)]
pub struct AdvisoryDispatchContinuation {
    tenant_id: Uuid,
    workspace_id: Uuid,
    actor_id: Uuid,
    opportunity_id: Uuid,
    dispatch_id: Uuid,
    capability: AdvisoryCapability,
    decision_point: AdvisoryDecisionPoint,
    target_kind: String,
    target_id: Option<Uuid>,
    work_revision: Option<i64>,
    material_digest: String,
    configuration_digest: String,
    request_sha256: String,
}

impl AdvisoryDispatchContinuation {
    pub(crate) fn after_committed_start(
        tenant: Uuid,
        workspace: Uuid,
        opportunity: &AdvisoryOpportunity,
        started: &AdvisoryDispatchStart,
        authorization: &AdvisoryDispatchAuthorization,
    ) -> Result<Self> {
        let reservation = started
            .budget_reservation
            .as_ref()
            .ok_or(Error::BudgetPolicyInvalid)?;
        let dispatch = &started.dispatch;
        let request_sha = format!("{:x}", Sha256::digest(&authorization.request_payload));
        let config_sha = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&authorization.configuration_snapshot)
                    .map_err(|_| Error::InputConflict)?
            )
        );
        if !started.should_send
            || dispatch.state != AdvisoryDispatchState::Sending
            || dispatch.send_certainty != AdvisorySendCertainty::SentUnknown
            || dispatch.id != authorization.dispatch_id
            || dispatch.opportunity_id != authorization.opportunity_id
            || dispatch.configuration_digest != authorization.configuration_digest
            || dispatch.material_digest != authorization.material_digest
            || dispatch.payload_digest != authorization.payload_digest
            || dispatch.provider != authorization.provider
            || dispatch.model != authorization.model
            || config_sha != authorization.configuration_digest
            || request_sha != authorization.payload_digest
            || reservation.dispatch_id != dispatch.id
            || reservation.request_sha256 != request_sha
            || reservation.request_utf8_bytes
                != i64::try_from(authorization.request_payload.len()).unwrap_or(-1)
            || reservation.reserved_calls != 1
        {
            return Err(Error::InputConflict);
        }
        Self::from_saved(tenant, workspace, opportunity, dispatch)
    }

    pub(crate) fn from_saved(
        tenant: Uuid,
        workspace: Uuid,
        opportunity: &AdvisoryOpportunity,
        dispatch: &AdvisoryDispatch,
    ) -> Result<Self> {
        if tenant.is_nil()
            || workspace.is_nil()
            || opportunity.id.is_nil()
            || dispatch.id.is_nil()
            || workspace != opportunity.workspace_id
            || opportunity.authorized_actor_id.is_nil()
            || dispatch.opportunity_id != opportunity.id
            || dispatch.material_digest != opportunity.material_digest
            || !matches!(
                dispatch.state,
                AdvisoryDispatchState::Sending | AdvisoryDispatchState::Sealed
            )
            || !supported_receipt_family(
                opportunity.capability,
                opportunity.decision_point,
                &opportunity.target_kind,
            )
            || opportunity.target_id.is_none()
        {
            return Err(Error::InputConflict);
        }
        Ok(Self {
            tenant_id: tenant,
            workspace_id: workspace,
            actor_id: opportunity.authorized_actor_id,
            opportunity_id: opportunity.id,
            dispatch_id: dispatch.id,
            capability: opportunity.capability,
            decision_point: opportunity.decision_point,
            target_kind: opportunity.target_kind.clone(),
            target_id: opportunity.target_id,
            work_revision: opportunity.work_revision,
            material_digest: opportunity.material_digest.clone(),
            configuration_digest: dispatch.configuration_digest.clone(),
            request_sha256: dispatch.payload_digest.clone(),
        })
    }
    pub fn tenant_id(&self) -> Uuid {
        self.tenant_id
    }
    pub fn workspace_id(&self) -> Uuid {
        self.workspace_id
    }
    pub fn actor_id(&self) -> Uuid {
        self.actor_id
    }
    pub fn opportunity_id(&self) -> Uuid {
        self.opportunity_id
    }
    pub fn dispatch_id(&self) -> Uuid {
        self.dispatch_id
    }
    pub fn capability(&self) -> AdvisoryCapability {
        self.capability
    }
    pub fn decision_point(&self) -> AdvisoryDecisionPoint {
        self.decision_point
    }
    pub fn target_kind(&self) -> &str {
        &self.target_kind
    }
    pub fn target_id(&self) -> Option<Uuid> {
        self.target_id
    }
    pub fn work_revision(&self) -> Option<i64> {
        self.work_revision
    }
    pub fn material_digest(&self) -> &str {
        &self.material_digest
    }
    pub fn configuration_digest(&self) -> &str {
        &self.configuration_digest
    }
    pub fn request_sha256(&self) -> &str {
        &self.request_sha256
    }
}

fn supported_receipt_family(
    capability: AdvisoryCapability,
    decision: AdvisoryDecisionPoint,
    target: &str,
) -> bool {
    matches!(
        (capability, decision, target),
        (
            AdvisoryCapability::EngineeringProfile,
            AdvisoryDecisionPoint::EngineeringProfileBeforeSelection,
            "matrix_task"
        ) | (
            AdvisoryCapability::ScopeDecomposition,
            AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection,
            "scope_candidate_set"
        ) | (
            AdvisoryCapability::PipelineRecommendation,
            AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen,
            "slice_candidate_node"
        )
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryProviderReceiptObservation {
    pub response_payload: Option<Vec<u8>>,
    pub http_status: Option<u16>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub response_complete: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AdvisoryProviderReceiptUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// Internal exact evidence; adapters must keep it off user audit responses.
pub struct StoredAdvisoryProviderReceipt {
    pub opportunity: AdvisoryOpportunity,
    pub dispatch: AdvisoryDispatch,
    pub configuration_snapshot: serde_json::Value,
    pub request_payload: Vec<u8>,
    pub request_payload_sha256: String,
    pub observation: Option<AdvisoryProviderReceiptObservation>,
    pub original_elapsed_ms: Option<i64>,
}
