use async_trait::async_trait;
use tect_domain::{
    AdvisoryDispatchOutcome, AdvisorySendCertainty, FrozenScopeSource,
    NormalizedScopeAdviceAnswers, Result, ScopeAdviceRequest, ScopeConstructorManifest,
    SourceObligation,
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopeAuthorityRequest {
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    pub session_id: Uuid,
    pub case_id: Uuid,
    pub candidate_set_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopeAuthorityObservation {
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    pub session_id: Uuid,
    pub case_id: Uuid,
    pub source: FrozenScopeSource,
    pub obligations: Vec<SourceObligation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopeAuthorizedInvalidObservation {
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    pub session_id: Uuid,
    pub case_id: Uuid,
    pub candidate_set_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScopeAuthorityOutcome {
    Authorized(ScopeAuthorityObservation),
    AuthorizedInvalid(ScopeAuthorizedInvalidObservation),
}

#[async_trait]
pub(crate) trait ScopeAuthorityObserver: Send + Sync {
    /// Authorization/inaccessibility remains an `Err` and creates no decision
    /// point. `AuthorizedInvalid` proves access while withholding unsafe source.
    async fn observe(&self, request: &ScopeAuthorityRequest) -> Result<ScopeAuthorityOutcome>;
}

#[async_trait]
pub(crate) trait ScopeManifestSupplier: Send + Sync {
    fn identity(&self) -> Option<(&'static str, &'static str)>;
    async fn supply(
        &self,
        observation: &ScopeAuthorityObservation,
    ) -> Result<ScopeConstructorManifest>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopeBudgetRequest {
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    pub case_id: Uuid,
    pub config_revision: i64,
    pub manifest_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopeBudgetPolicyEvaluation {
    pub policy_id: String,
}

#[async_trait]
pub(crate) trait ScopeBudgetPolicy: Send + Sync {
    /// Pure owner policy evaluation: implementations must not reserve, charge,
    /// release, or mutate budget state.
    async fn evaluate(
        &self,
        request: &ScopeBudgetRequest,
    ) -> Result<Option<ScopeBudgetPolicyEvaluation>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeAdviceProviderRequest {
    pub dispatch_id: Uuid,
    pub request: ScopeAdviceRequest,
    pub(crate) budget_policy: ScopeBudgetPolicyEvaluation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeAdviceProviderObservation {
    pub send_certainty: AdvisorySendCertainty,
    pub outcome: AdvisoryDispatchOutcome,
    pub answers: Option<NormalizedScopeAdviceAnswers>,
    pub response_payload: Option<Vec<u8>>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub latency_ms: Option<i64>,
    pub raw_response_ref: Option<String>,
    pub failure_reason: Option<ScopeAdviceProviderFailureReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeAdviceProviderFailureReason {
    HttpStatus,
    InvalidContentType,
    ResponseOversize,
    ResponseBodyRead,
    InvalidResponse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeAdviceProviderError {
    /// Disabled adapter or preflight rejection proven to precede any send attempt.
    ProvenNotSent,
    /// Timeout, connection loss after an attempt, or any other uncertain send result.
    SentUnknown {
        raw_response_ref: Option<String>,
        latency_ms: i64,
    },
}

#[async_trait]
pub trait ScopeAdviceProvider: Send + Sync {
    fn identity(&self) -> Option<(&'static str, &'static str)>;
    /// `Ok` is reserved for a transport result proven sent, including typed
    /// provider/body failures. Pre-response uncertainty uses the error variant.
    async fn attempt(
        &self,
        request: &ScopeAdviceProviderRequest,
    ) -> std::result::Result<ScopeAdviceProviderObservation, ScopeAdviceProviderError>;
}

#[async_trait]
pub(crate) trait ScopeCaller: Send + Sync {
    async fn call(&self) -> Result<()>;
}
#[async_trait]
pub(crate) trait ScopeVerifier: Send + Sync {
    async fn verify(&self) -> Result<()>;
}

#[derive(Debug, Default)]
pub(crate) struct UnavailableScopeAuthorityObserver;
#[async_trait]
impl ScopeAuthorityObserver for UnavailableScopeAuthorityObserver {
    async fn observe(&self, _: &ScopeAuthorityRequest) -> Result<ScopeAuthorityOutcome> {
        Err(tect_domain::Error::TransportUnavailable)
    }
}

#[derive(Debug, Default)]
pub(crate) struct UnavailableScopeManifestSupplier;
#[async_trait]
impl ScopeManifestSupplier for UnavailableScopeManifestSupplier {
    fn identity(&self) -> Option<(&'static str, &'static str)> {
        None
    }
    async fn supply(&self, _: &ScopeAuthorityObservation) -> Result<ScopeConstructorManifest> {
        Err(tect_domain::Error::TransportUnavailable)
    }
}

#[derive(Debug, Default)]
pub(crate) struct DenyScopeBudget;
#[async_trait]
impl ScopeBudgetPolicy for DenyScopeBudget {
    async fn evaluate(
        &self,
        _: &ScopeBudgetRequest,
    ) -> Result<Option<ScopeBudgetPolicyEvaluation>> {
        Ok(None)
    }
}

#[derive(Debug, Default)]
pub(crate) struct DisabledScopeAdviceProvider;
#[async_trait]
impl ScopeAdviceProvider for DisabledScopeAdviceProvider {
    fn identity(&self) -> Option<(&'static str, &'static str)> {
        None
    }
    async fn attempt(
        &self,
        _: &ScopeAdviceProviderRequest,
    ) -> std::result::Result<ScopeAdviceProviderObservation, ScopeAdviceProviderError> {
        Err(ScopeAdviceProviderError::ProvenNotSent)
    }
}

#[derive(Debug, Default)]
pub(crate) struct DisabledScopeCaller;
#[async_trait]
impl ScopeCaller for DisabledScopeCaller {
    async fn call(&self) -> Result<()> {
        Err(tect_domain::Error::TransportUnavailable)
    }
}

#[derive(Debug, Default)]
pub(crate) struct DisabledScopeVerifier;
#[async_trait]
impl ScopeVerifier for DisabledScopeVerifier {
    async fn verify(&self) -> Result<()> {
        Err(tect_domain::Error::TransportUnavailable)
    }
}
