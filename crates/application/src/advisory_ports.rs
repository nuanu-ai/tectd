use crate::{MatrixTaskRevision, canonical_matrix_input_digest};
use async_trait::async_trait;
use sha2::Digest;
use tect_domain::{
    AdvisoryAuditPage, AdvisoryAuditQuery, AdvisoryDispatch, AdvisoryDispatchAuthorization,
    AdvisoryDispatchCancellation, AdvisoryDispatchOutcome, AdvisoryDispatchSeal,
    AdvisoryDispatchStart, AdvisoryModelConfiguration, AdvisoryOpportunity,
    AdvisoryOpportunityDetail, AdvisoryOpportunityInput, AdvisoryProviderProfileRef,
    AdvisoryReconciliationEvidence, AdvisorySendCertainty, ConfigureWorkspaceAdvisory,
    EngineeringMatrixComposition, Error, MatrixAdviceEligibility, MatrixRanking,
    MatrixSourceVerificationStatus, Result, WorkspaceAdvisoryConfig, matrix_evaluation_digest,
};
use uuid::Uuid;

/// Unforgeable outside this crate. Persistence adapters accept it so their
/// public cross-crate trait cannot become a public dispatch-initiation API.
#[doc(hidden)]
pub struct AdvisoryLifecycleCapability(());

impl AdvisoryLifecycleCapability {
    pub(crate) const fn internal() -> Self {
        Self(())
    }
}

/// Slice 0 persistence boundary. The provider is deliberately absent here:
/// every provider send must be authorized and recorded through a later
/// dispatch use case, while no-call opportunities are still durable.
#[async_trait]
pub trait AdvisoryStore: Send {
    async fn finalize_pipeline_advisory_without_advice(
        &mut self,
        _workspace_id: Uuid,
        _opportunity_id: Uuid,
        _expected_config_revision: i64,
        _dispatch: &AdvisoryDispatch,
    ) -> Result<AdvisoryOpportunity> {
        Err(Error::Forbidden)
    }
    /// Authorized-actor internal recovery evidence, never a public audit body.
    async fn advisory_dispatch_receipt(
        &mut self,
        _workspace_id: Uuid,
        _opportunity_id: Uuid,
    ) -> Result<Option<crate::StoredAdvisoryProviderReceipt>> {
        Err(Error::Forbidden)
    }
    async fn advisory_config(&mut self, workspace_id: Uuid) -> Result<WorkspaceAdvisoryConfig>;
    async fn materialize_advisory_config(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
    ) -> Result<WorkspaceAdvisoryConfig>;
    async fn configure_advisory(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &ConfigureWorkspaceAdvisory,
    ) -> Result<WorkspaceAdvisoryConfig>;
    async fn capture_advisory_opportunity(
        &mut self,
        workspace_id: Uuid,
        input: &AdvisoryOpportunityInput,
    ) -> Result<AdvisoryOpportunity>;
    async fn advisory_opportunity_for_dispatch(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<AdvisoryOpportunity>;
    async fn advisory_opportunity_by_request(
        &mut self,
        workspace_id: Uuid,
        request_key: &str,
    ) -> Result<Option<AdvisoryOpportunity>>;
    /// Internal recovery evidence for one Matrix attempt. `None` resolves only
    /// the initial attempt for a known opportunity; `Some` requires an exact
    /// dispatch ID. The capability keeps bytes off the general audit surface.
    async fn matrix_dispatch_for_recovery(
        &mut self,
        capability: &AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        actor_id: Uuid,
        opportunity_id: Uuid,
        dispatch_id: Option<Uuid>,
    ) -> Result<StoredMatrixDispatch>;
    async fn authorize_advisory_dispatch(
        &mut self,
        capability: &AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        expected_config_revision: i64,
        dispatch: &AdvisoryDispatchAuthorization,
    ) -> Result<AdvisoryDispatch>;
    async fn start_advisory_dispatch(
        &mut self,
        capability: &AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        dispatch_id: Uuid,
    ) -> Result<AdvisoryDispatchStart>;
    /// The Matrix-specific gate consumes the application's fresh external
    /// evidence decision in the same transaction as the committed send start.
    async fn start_verified_matrix_dispatch(
        &mut self,
        capability: &AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        dispatch_id: Uuid,
        verification_current: bool,
    ) -> Result<AdvisoryDispatchStart>;
    async fn seal_advisory_dispatch(
        &mut self,
        capability: &AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        seal: &AdvisoryDispatchSeal,
    ) -> Result<AdvisoryDispatch>;
    /// Called in a new transaction after the raw seal has committed.
    async fn consume_advisory_budget(
        &mut self,
        capability: &AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        dispatch_id: Uuid,
    ) -> Result<tect_domain::AdvisoryBudgetConsumption>;
    async fn cancel_advisory_dispatch(
        &mut self,
        capability: &AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        dispatch_id: Uuid,
    ) -> Result<AdvisoryDispatchCancellation>;
    async fn reconcile_advisory_dispatch(
        &mut self,
        capability: &AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        evidence: &AdvisoryReconciliationEvidence,
    ) -> Result<AdvisoryDispatch>;
    async fn finalize_advisory_opportunity(
        &mut self,
        capability: &AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        opportunity_id: Uuid,
        expected_config_revision: i64,
        dispatch: &AdvisoryDispatch,
    ) -> Result<AdvisoryOpportunity>;
    async fn advisory_audit(
        &mut self,
        workspace_id: Uuid,
        scope_id: Option<Uuid>,
        query: &AdvisoryAuditQuery,
    ) -> Result<AdvisoryAuditPage>;
    async fn advisory_opportunity_detail(
        &mut self,
        workspace_id: Uuid,
        scope_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<AdvisoryOpportunityDetail>;
    async fn advisory_candidate_set_exists(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
    ) -> Result<bool>;
    async fn candidate_advisory_audit(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        query: &AdvisoryAuditQuery,
    ) -> Result<AdvisoryAuditPage>;
    async fn candidate_advisory_opportunity_detail(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<AdvisoryOpportunityDetail>;
}

/// Persisted Matrix attempt, including exact transport bytes. Never serialize
/// this into an MCP or audit response.
#[derive(Clone, PartialEq, Eq)]
pub struct StoredMatrixDispatch {
    pub dispatch: AdvisoryDispatch,
    pub binding: MatrixProviderBinding,
    pub provider_profile_ref: AdvisoryProviderProfileRef,
    pub model_configuration: AdvisoryModelConfiguration,
    pub configuration_snapshot: serde_json::Value,
    pub destination: String,
    pub wire_version: String,
    pub request_payload: Vec<u8>,
    pub request_payload_sha256: String,
    pub response_payload: Option<Vec<u8>>,
    pub response_payload_sha256: Option<String>,
    pub response_http_status: Option<u16>,
    pub original_input_tokens: Option<u64>,
    pub original_output_tokens: Option<u64>,
    pub original_elapsed_ms: Option<i64>,
    pub raw_observation_sealed: bool,
    pub response_complete: bool,
    pub original_transport_context: Option<crate::AdvisoryProviderTransportContext>,
}

impl std::fmt::Debug for StoredMatrixDispatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredMatrixDispatch")
            .field("dispatch", &self.dispatch)
            .field("binding", &self.binding)
            .field("provider_profile_ref", &self.provider_profile_ref)
            .field("model_configuration", &self.model_configuration)
            .field("destination", &self.destination)
            .field("wire_version", &self.wire_version)
            .field("request_payload_sha256", &self.request_payload_sha256)
            .field("response_payload_sha256", &self.response_payload_sha256)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct AdvisoryProviderRequest {
    pub dispatch_id: Uuid,
    pub provider_profile_ref: AdvisoryProviderProfileRef,
    pub model_configuration: AdvisoryModelConfiguration,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct AdvisoryProviderObservation {
    pub send_certainty: AdvisorySendCertainty,
    pub outcome: AdvisoryDispatchOutcome,
    pub response_payload: Option<Vec<u8>>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub raw_response_ref: Option<String>,
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControlledAdvisoryDispatch {
    pub dispatch_id: Uuid,
    pub opportunity_id: Uuid,
    pub predecessor_dispatch_id: Option<Uuid>,
    pub attempt_number: i32,
    pub retry_basis: tect_domain::AdvisoryRetryBasis,
    pub payload: Vec<u8>,
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControlledAdvisoryResult {
    pub opportunity_id: Uuid,
    pub dispatch: AdvisoryDispatch,
    pub advice_eligible: bool,
}

#[async_trait]
#[allow(dead_code)]
pub(crate) trait AdvisoryProvider: Send + Sync {
    fn identity(&self) -> Option<(&'static str, &'static str)>;
    async fn attempt(
        &self,
        request: &AdvisoryProviderRequest,
    ) -> Result<AdvisoryProviderObservation>;
}

#[derive(Debug, Default)]
pub(crate) struct DisabledAdvisoryProvider;

#[async_trait]
impl AdvisoryProvider for DisabledAdvisoryProvider {
    fn identity(&self) -> Option<(&'static str, &'static str)> {
        None
    }

    async fn attempt(&self, _: &AdvisoryProviderRequest) -> Result<AdvisoryProviderObservation> {
        Err(tect_domain::Error::TransportUnavailable)
    }
}

/// Exact, immutable material offered to a Matrix ranking provider. This is a
/// separate port from the Scope advisory dispatch protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixProviderBinding {
    pub task_id: Uuid,
    pub task_revision: i64,
    pub input_digest: String,
    pub choice_set_id: String,
    pub choice_set_version: u64,
    pub choice_set_digest: String,
    pub evaluation_digest: String,
    /// Present only for a v2 positive request built from revalidated evidence.
    pub verification_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixProviderRequest {
    binding: MatrixProviderBinding,
    revision: MatrixTaskRevision,
    /// Includes mandatory cards, unresolved evidence, and source provenance.
    composition: EngineeringMatrixComposition,
    provider_profile_ref: AdvisoryProviderProfileRef,
    model_configuration: AdvisoryModelConfiguration,
    eligibility: MatrixAdviceEligibility,
}

/// Minted only by the application after the latest stored verification and
/// every bound evidence item are revalidated for the saved revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevalidatedMatrixVerification {
    validated: tect_domain::ValidatedMatrixVerification,
}

impl RevalidatedMatrixVerification {
    pub(crate) fn from_revalidated(validated: tect_domain::ValidatedMatrixVerification) -> Self {
        Self { validated }
    }

    pub fn record_digest(&self) -> &str {
        self.validated.record_digest()
    }

    pub fn disposition_digest(
        &self,
        input: &tect_domain::EngineeringMatrixInput,
        composition: &EngineeringMatrixComposition,
        choice_set: &tect_domain::EngineeringChoiceSet,
    ) -> Result<String> {
        tect_domain::matrix_verified_disposition_digest(
            input,
            composition,
            choice_set,
            &self.validated,
        )
    }
}

include!("advisory_ports/matrix_request.rs");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixProviderResponse {
    pub binding: MatrixProviderBinding,
    pub provider_profile_ref: AdvisoryProviderProfileRef,
    pub model_configuration: AdvisoryModelConfiguration,
    /// Original opaque bytes received from the provider, before parsing or
    /// normalization. Generic dispatch persistence stores these bytes in the
    /// dispatch row; guarded advice binds to that row by dispatch ID and hash.
    pub raw_response_payload: Vec<u8>,
    pub response_payload_sha256: String,
    pub ranking: MatrixRanking,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// Opaque transport evidence. No ranking interpretation occurs at this seam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixProviderObservation {
    pub response_payload: Option<Vec<u8>>,
    pub http_status: Option<u16>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// Compatibility only: native transports leave this absent. Never persisted.
    pub legacy_response: Option<MatrixProviderResponse>,
    pub response_complete: bool,
    pub original_transport_context: Option<crate::AdvisoryProviderTransportContext>,
}

pub type MatrixProviderUsage = crate::AdvisoryProviderReceiptUsage;

impl From<&MatrixProviderObservation> for crate::AdvisoryProviderReceiptObservation {
    fn from(value: &MatrixProviderObservation) -> Self {
        Self {
            response_payload: value.response_payload.clone(),
            http_status: value.http_status,
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            response_complete: value.response_complete,
            original_transport_context: value.original_transport_context.clone(),
        }
    }
}

impl MatrixProviderResponse {
    pub fn validate_for(&self, request: &MatrixProviderRequest) -> Result<()> {
        if self.binding != request.binding
            || self.provider_profile_ref != request.provider_profile_ref
            || self.model_configuration != request.model_configuration
            || self.raw_response_payload.is_empty()
            || self.response_payload_sha256
                != format!("{:x}", sha2::Sha256::digest(&self.raw_response_payload))
        {
            return Err(Error::InvalidArguments);
        }
        self.ranking.validate(&request.eligibility)
    }
}

#[async_trait]
pub trait MatrixAdviceProvider: Send + Sync {
    /// A disabled or unconfigured provider has no identity and cannot prepare a send.
    fn identity(&self) -> Option<crate::MatrixProviderIdentity>;

    /// Pure serialization and target binding. No transport or budget mutation.
    fn prepare(
        &self,
        request: &MatrixProviderRequest,
    ) -> Result<crate::PreparedMatrixAdviceAttempt>;

    /// Interpret an already sealed, persisted response against the exact
    /// prepared request. This is pure: it cannot start or repeat a send.
    /// Providers without an explicit saved-response parser fail closed.
    fn parse_sealed_response(
        &self,
        _request: &MatrixProviderRequest,
        _saved: &StoredMatrixDispatch,
    ) -> Result<MatrixProviderResponse> {
        Err(Error::TransportUnavailable)
    }

    fn sealed_response_usage(&self, saved: &StoredMatrixDispatch) -> MatrixProviderUsage {
        MatrixProviderUsage {
            input_tokens: saved.original_input_tokens,
            output_tokens: saved.original_output_tokens,
        }
    }

    async fn observe_prepared(
        &self,
        prepared: crate::PreparedMatrixAdviceAttempt,
        permit: crate::MatrixStartedDispatchPermit,
    ) -> Result<MatrixProviderObservation> {
        self.attempt_prepared(prepared, permit)
            .await
            .map(|response| MatrixProviderObservation {
                response_payload: Some(response.raw_response_payload.clone()),
                http_status: None,
                input_tokens: response.input_tokens,
                output_tokens: response.output_tokens,
                legacy_response: Some(response),
                response_complete: true,
                original_transport_context: None,
            })
    }

    /// Transport consumes the exact bytes that were prepared before authorization.
    async fn attempt_prepared(
        &self,
        prepared: crate::PreparedMatrixAdviceAttempt,
        permit: crate::MatrixStartedDispatchPermit,
    ) -> Result<MatrixProviderResponse>;
}

#[derive(Debug, Default)]
pub struct DisabledMatrixAdviceProvider;

#[cfg(test)]
#[path = "advisory_ports/matrix_observation_tests.rs"]
mod matrix_observation_tests;

#[async_trait]
impl MatrixAdviceProvider for DisabledMatrixAdviceProvider {
    fn identity(&self) -> Option<crate::MatrixProviderIdentity> {
        None
    }

    fn prepare(&self, _: &MatrixProviderRequest) -> Result<crate::PreparedMatrixAdviceAttempt> {
        Err(Error::TransportUnavailable)
    }

    async fn attempt_prepared(
        &self,
        _: crate::PreparedMatrixAdviceAttempt,
        _: crate::MatrixStartedDispatchPermit,
    ) -> Result<MatrixProviderResponse> {
        Err(Error::TransportUnavailable)
    }
}
