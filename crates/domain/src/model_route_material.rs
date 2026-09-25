//! Immutable model-route recommendation, attempt, and decision material.
use crate::{
    AdvisoryRequestPreference, EligibleModelRoutes, ModelRouteCatalogue, ModelRouteRanking,
    ModelRouteRecord, ModelRouteWorkContext,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelRouteAttemptState {
    NoCall,
    SendUnknown,
    RawSealed,
    Parsed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelRouteAttemptSnapshot {
    pub attempt_id: Uuid,
    pub state: ModelRouteAttemptState,
    pub no_call_reason: Option<String>,
    pub request_sha256: Option<String>,
    pub response_sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModelRoutePreparation {
    Prepared,
    WorkspaceDisabled,
    SessionSkip,
    RequestSkip,
    CapabilityUnavailable,
    UnknownWorkFacts,
    NoEligibleRoutes,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreparedModelRouteRecommendation {
    pub workspace_id: Uuid,
    pub request_key: String,
    pub session_preference: AdvisoryRequestPreference,
    pub request_preference: AdvisoryRequestPreference,
    pub advisory_config_revision: i64,
    pub work: ModelRouteWorkContext,
    pub catalogue: Option<ModelRouteCatalogue>,
    pub eligible: Option<EligibleModelRoutes>,
    pub preparation: ModelRoutePreparation,
    /// Recommendation stays empty until a separately validated ranking arrives.
    pub routes: ModelRouteRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModelRouteDecisionInput {
    /// Accepted only when the decision store returns exact sealed provider
    /// evidence; the caller cannot turn arbitrary IDs into Jev advice.
    Ranking(ModelRouteRanking),
    Abstain,
    NoCall,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModelRouteAbstainReason {
    Explicit,
    ProviderNoPreference,
    ProviderInsufficientEvidence,
    EmptyRanking,
    NoCall,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModelRouteDecisionOutcome {
    Recommended { route_id: String },
    Abstained { reason: ModelRouteAbstainReason },
    NoRoute { reason: ModelRoutePreparation },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapturedModelRouteDecision {
    pub id: Uuid,
    pub prepared: PreparedModelRouteRecommendation,
    pub input: ModelRouteDecisionInput,
    pub outcome: ModelRouteDecisionOutcome,
    pub routes: ModelRouteRecord,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModelRouteDispositionAction {
    Accept,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapturedModelRouteDisposition {
    pub id: Uuid,
    pub decision_id: Uuid,
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    pub action: ModelRouteDispositionAction,
    pub rationale: String,
}

/// Current authorized view, serialized for host responses only.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModelRouteView {
    pub preparation: PreparedModelRouteRecommendation,
    pub attempt: Option<ModelRouteAttemptSnapshot>,
    pub decision: Option<CapturedModelRouteDecision>,
    pub disposition: Option<CapturedModelRouteDisposition>,
}
