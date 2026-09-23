use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use tect_domain::{
    AdvisoryDispatchOutcome, AdvisorySendCertainty, FrozenScopeSource,
    NormalizedScopeAdviceAnswers, Result, ScopeAdviceRequest, ScopeConstructorManifest,
    SourceObligation,
};
use uuid::Uuid;

/// Caller-authored alternatives are request-local until resolved against
/// the authoritative candidate snapshot and its persisted source fragments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoredScopeAlternative {
    pub key: String,
    pub kind: tect_domain::ScopeDecompositionKind,
    pub draft: tect_domain::ScopeCandidateDraft,
    pub covered_source_ref_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoredScopeSet {
    pub expected_candidate_set_revision: i64,
    pub baseline_key: String,
    pub alternatives: Vec<AuthoredScopeAlternative>,
}

impl AuthoredScopeSet {
    pub fn validate(&self) -> Result<()> {
        if self.expected_candidate_set_revision < 1
            || self.alternatives.is_empty()
            || self.alternatives.len() > 100
        {
            return Err(tect_domain::Error::InvalidArguments);
        }
        let mut keys = BTreeSet::new();
        for alternative in &self.alternatives {
            if !valid_request_local_key(&alternative.key)
                || !keys.insert(&alternative.key)
                || alternative.covered_source_ref_ids.iter().any(Uuid::is_nil)
                || alternative
                    .covered_source_ref_ids
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1])
            {
                return Err(tect_domain::Error::InvalidArguments);
            }
            alternative.draft.validate()?;
        }
        if !keys.contains(&self.baseline_key) {
            return Err(tect_domain::Error::InvalidArguments);
        }
        Ok(())
    }
}

fn valid_request_local_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeAuthorityRequest {
    pub tenant_id: Uuid,
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    pub session_id: Uuid,
    pub candidate_set_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeAuthorityObservation {
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    pub session_id: Uuid,
    pub candidate_set_id: Uuid,
    pub source: FrozenScopeSource,
    pub obligations: Vec<SourceObligation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeAuthorizedInvalidObservation {
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    pub session_id: Uuid,
    pub candidate_set_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeAuthorityOutcome {
    Authorized(ScopeAuthorityObservation),
    AuthorizedInvalid(ScopeAuthorizedInvalidObservation),
}

#[async_trait]
pub trait ScopeAuthorityObserver: Send + Sync {
    /// Authorization/inaccessibility remains an `Err` and creates no decision
    /// point. `AuthorizedInvalid` proves access while withholding unsafe source.
    async fn observe(&self, request: &ScopeAuthorityRequest) -> Result<ScopeAuthorityOutcome>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeAuthoredManifestRequest {
    pub tenant_id: Uuid,
    pub observation: ScopeAuthorityObservation,
    pub authored_scope_set: AuthoredScopeSet,
}

#[async_trait]
pub trait ScopeManifestSupplier: Send + Sync {
    fn identity(&self) -> Option<(&'static str, &'static str)>;
    async fn supply(
        &self,
        observation: &ScopeAuthorityObservation,
    ) -> Result<ScopeConstructorManifest>;
    async fn supply_authored(
        &self,
        request: &ScopeAuthoredManifestRequest,
    ) -> Result<ScopeConstructorManifest> {
        let _ = request;
        Err(tect_domain::Error::InputPending)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopeBudgetRequest {
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    pub candidate_set_id: Uuid,
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

/// Immutable, application-owned representation of the exact request body and
/// provider target prepared before dispatch authorization. It contains no
/// credentials and can only be consumed, not mutated.
#[derive(PartialEq, Eq)]
pub struct PreparedScopeAdviceAttempt {
    request: ScopeAdviceRequest,
    body: Vec<u8>,
    profile: String,
    model: String,
    destination: String,
    wire_version: String,
    body_length: usize,
    body_sha256: String,
}

impl std::fmt::Debug for PreparedScopeAdviceAttempt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedScopeAdviceAttempt")
            .field("body", &"[redacted]")
            .field("body_length", &self.body_length)
            .finish_non_exhaustive()
    }
}

impl PreparedScopeAdviceAttempt {
    pub fn new(
        request: ScopeAdviceRequest,
        body: Vec<u8>,
        profile: String,
        model: String,
        destination: String,
        wire_version: String,
    ) -> std::result::Result<Self, ScopeAdviceProviderError> {
        if profile.is_empty()
            || model.is_empty()
            || destination.is_empty()
            || wire_version.is_empty()
            || std::str::from_utf8(&body).is_err()
        {
            return Err(ScopeAdviceProviderError::ProvenNotSent);
        }
        let body_length = body.len();
        let body_sha256 = format!("{:x}", Sha256::digest(&body));
        Ok(Self {
            request,
            body,
            profile,
            model,
            destination,
            wire_version,
            body_length,
            body_sha256,
        })
    }

    pub fn request(&self) -> &ScopeAdviceRequest {
        &self.request
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn profile(&self) -> &str {
        &self.profile
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn destination(&self) -> &str {
        &self.destination
    }

    pub fn wire_version(&self) -> &str {
        &self.wire_version
    }

    pub fn body_length(&self) -> usize {
        self.body_length
    }

    pub fn body_sha256(&self) -> &str {
        &self.body_sha256
    }

    /// Consume the prepared value at the transport boundary while preserving
    /// ownership of the exact body allocation created during preparation.
    pub fn into_parts(
        self,
    ) -> (
        ScopeAdviceRequest,
        Vec<u8>,
        String,
        String,
        String,
        String,
        usize,
        String,
    ) {
        (
            self.request,
            self.body,
            self.profile,
            self.model,
            self.destination,
            self.wire_version,
            self.body_length,
            self.body_sha256,
        )
    }
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
    /// Pure, no-I/O serialization and provider-target binding. Rejections are
    /// proven not sent and must be recorded as a no-call before authorization.
    fn prepare(
        &self,
        request: &ScopeAdviceRequest,
    ) -> std::result::Result<PreparedScopeAdviceAttempt, ScopeAdviceProviderError>;
    /// `Ok` is reserved for a transport result proven sent, including typed
    /// provider/body failures. Pre-response uncertainty uses the error variant.
    async fn attempt_prepared(
        &self,
        request: &ScopeAdviceProviderRequest,
        prepared: PreparedScopeAdviceAttempt,
        permit: crate::scope_advisory_orchestration::StartedScopeDispatchPermit,
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
    fn prepare(
        &self,
        _: &ScopeAdviceRequest,
    ) -> std::result::Result<PreparedScopeAdviceAttempt, ScopeAdviceProviderError> {
        Err(ScopeAdviceProviderError::ProvenNotSent)
    }
    async fn attempt_prepared(
        &self,
        _: &ScopeAdviceProviderRequest,
        _: PreparedScopeAdviceAttempt,
        _: crate::scope_advisory_orchestration::StartedScopeDispatchPermit,
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
