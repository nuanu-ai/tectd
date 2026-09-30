//! Trusted server boundary: pointers and payload claims are not approval.
use async_trait::async_trait;
use tect_domain::{
    DeliveryApproach, DeliveryMechanismDecisionCard, EngineeringCandidate, EngineeringChoiceSet,
    Result, TechnicalDecisionFact, TechnicalOwnerApprovalClaim,
};
use uuid::Uuid;

pub const TECHNICAL_DECISION_VALIDATOR_POLICY_VERSION: &str =
    "tect.matrix-technical-decision-policy/1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechnicalDecisionEvidenceReference {
    pub artifact_id: Uuid,
    pub artifact_version: i64,
    pub content_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerTechnicalDecisionTaskBinding {
    pub tenant_id: Uuid,
    pub workspace_id: Uuid,
    pub task_id: Uuid,
    pub task_revision: i64,
    pub operating_verification_digest: String,
    pub operating_policy_version: String,
    pub requirements_binding: crate::MatrixTaskRequirementsBinding,
    pub choice_set: EngineeringChoiceSet,
    pub choice_set_digest: String,
    pub recorded_by_principal_id: Uuid,
}

/// Exact frozen prose plus explicitly approved structured meaning. Never
/// infer kind, outcome or operations from the legacy alternative's prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechnicalDecisionCandidateMapping {
    pub frozen_candidate: EngineeringCandidate,
    pub technical_approach: DeliveryApproach,
}

/// Authenticated server metadata. Delegated technical approval is not proof
/// of owner authorship; requirements confirmation is not technical approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechnicalDecisionApprovalRecord {
    pub claim: TechnicalOwnerApprovalClaim,
    pub card_digest: String,
    pub candidate_digest: String,
    pub choice_set_digest: String,
    pub recorded_by_principal_id: Uuid,
    pub owner_author_principal_id: Uuid,
    pub owner_authorship_ref: String,
}

/// A single immutable read of approved source and independent metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTechnicalDecisionEvidence {
    pub binding: ServerTechnicalDecisionTaskBinding,
    pub reference: TechnicalDecisionEvidenceReference,
    pub card: DeliveryMechanismDecisionCard,
    pub facts: Vec<TechnicalDecisionFact>,
    pub approval: TechnicalDecisionApprovalRecord,
    pub candidate_mapping: Vec<TechnicalDecisionCandidateMapping>,
    pub validator_policy_version: String,
    pub max_age_seconds: i64,
}

#[async_trait]
pub trait TechnicalDecisionEvidenceResolver: Send + Sync {
    /// Independently authenticate tenant/workspace, pinned bytes/version,
    /// approval scope, validation policy and owner authorship before Some.
    /// Unknown provenance returns None; payload Accepted flags never suffice.
    async fn resolve(
        &self,
        binding: &ServerTechnicalDecisionTaskBinding,
        reference: &TechnicalDecisionEvidenceReference,
        now: i64,
    ) -> Result<Option<ResolvedTechnicalDecisionEvidence>>;
}

pub struct DisabledTechnicalDecisionEvidenceResolver;
#[async_trait]
impl TechnicalDecisionEvidenceResolver for DisabledTechnicalDecisionEvidenceResolver {
    async fn resolve(
        &self,
        _: &ServerTechnicalDecisionTaskBinding,
        _: &TechnicalDecisionEvidenceReference,
        _: i64,
    ) -> Result<Option<ResolvedTechnicalDecisionEvidence>> {
        Ok(None)
    }
}
