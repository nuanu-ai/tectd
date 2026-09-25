//! Optional adviser boundary. A route recommendation never dispatches that route.
use async_trait::async_trait;
use tect_domain::{
    AdvisoryBudgetPolicy, Error, ModelRouteRanking, ModelRouteRankingWireOutcome,
    ModelRouteRankingWireRequest, Result, model_route_ranking_from_wire, model_route_wire_sha256,
    parse_model_route_ranking_response,
};
use uuid::Uuid;

use crate::PreparedModelRouteRecommendation;

pub use tect_domain::{ModelRouteAttemptSnapshot, ModelRouteAttemptState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRouteRunNoCall {
    Preparation(crate::ModelRoutePreparation),
    ProviderUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRoutePreparedAttempt {
    pub request: ModelRouteRankingWireRequest,
    pub request_bytes: Vec<u8>,
    pub request_sha256: String,
}

impl ModelRoutePreparedAttempt {
    pub fn new(request: ModelRouteRankingWireRequest) -> Result<Self> {
        let request_bytes = request.bytes()?;
        let request_sha256 = model_route_wire_sha256(&request_bytes);
        Ok(Self {
            request,
            request_bytes,
            request_sha256,
        })
    }

    pub fn verify(&self, prepared: &PreparedModelRouteRecommendation) -> Result<()> {
        let catalogue = prepared.catalogue.as_ref().ok_or(Error::InputConflict)?;
        let eligible = prepared.eligible.as_ref().ok_or(Error::InputConflict)?;
        let expected = ModelRouteRankingWireRequest::new(
            prepared.workspace_id,
            &prepared.request_key,
            &prepared.work,
            catalogue,
            eligible,
            &self.request.binding.adviser_model,
        )?;
        if self.request != expected
            || self.request_bytes != self.request.bytes()?
            || self.request_sha256 != model_route_wire_sha256(&self.request_bytes)
        {
            return Err(Error::InputConflict);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRouteSendPermit {
    pub attempt_id: Uuid,
    pub workspace_id: Uuid,
    pub preparation_request_key: String,
    pub request_sha256: String,
    pub policy_id: Uuid,
    pub policy_version: i64,
    pub policy_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRouteProviderObservation {
    pub raw: Vec<u8>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub elapsed_monotonic_ms: Option<i64>,
}

/// The currently authenticated invocation, distinct from the historical
/// caller that authored the selected Matrix/Work save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelRouteInvocation {
    pub session_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRouteSealedRankingEvidence {
    pub permit: ModelRouteSendPermit,
    pub attempted: ModelRoutePreparedAttempt,
    pub raw_response: Vec<u8>,
    pub response_sha256: String,
    pub outcome: ModelRouteRankingWireOutcome,
}

impl ModelRouteSealedRankingEvidence {
    /// Rechecks both frozen bytes and the full saved Work/catalogue/host binding.
    pub fn verify(
        &self,
        prepared: &PreparedModelRouteRecommendation,
    ) -> Result<Option<ModelRouteRanking>> {
        self.attempted.verify(prepared)?;
        if self.permit.attempt_id.is_nil()
            || self.permit.policy_id.is_nil()
            || self.permit.policy_version <= 0
            || self.permit.policy_digest.len() != 64
            || self.permit.workspace_id != prepared.workspace_id
            || self.permit.preparation_request_key != prepared.request_key
            || self.permit.request_sha256 != self.attempted.request_sha256
            || self.response_sha256 != model_route_wire_sha256(&self.raw_response)
            || parse_model_route_ranking_response(&self.attempted.request, &self.raw_response)?
                != self.outcome
        {
            return Err(Error::InputConflict);
        }
        Ok(model_route_ranking_from_wire(
            &self.attempted.request,
            &self.outcome,
        ))
    }
}

/// The default is disabled. A fake or trusted host adapter must be explicitly installed.
#[async_trait]
pub trait ModelRouteRankingProvider: Send + Sync {
    fn available(&self) -> bool {
        true
    }
    fn prepare(
        &self,
        saved: &PreparedModelRouteRecommendation,
    ) -> Result<ModelRoutePreparedAttempt>;
    async fn attempt_prepared(
        &self,
        attempted: ModelRoutePreparedAttempt,
        permit: ModelRouteSendPermit,
    ) -> Result<Vec<u8>>;
    /// Existing adapters provide no token evidence and therefore cannot publish
    /// a recommendation. An adapter with trusted usage evidence overrides this.
    async fn attempt_prepared_observed(
        &self,
        attempted: ModelRoutePreparedAttempt,
        permit: ModelRouteSendPermit,
    ) -> Result<ModelRouteProviderObservation> {
        let start = std::time::Instant::now();
        let raw = self.attempt_prepared(attempted, permit).await?;
        let elapsed_monotonic_ms = i64::try_from(start.elapsed().as_millis()).ok();
        Ok(ModelRouteProviderObservation {
            raw,
            input_tokens: None,
            output_tokens: None,
            elapsed_monotonic_ms,
        })
    }
}

pub struct DisabledModelRouteRankingProvider;

#[async_trait]
impl ModelRouteRankingProvider for DisabledModelRouteRankingProvider {
    fn available(&self) -> bool {
        false
    }
    fn prepare(&self, _: &PreparedModelRouteRecommendation) -> Result<ModelRoutePreparedAttempt> {
        Err(Error::TransportUnavailable)
    }
    async fn attempt_prepared(
        &self,
        _: ModelRoutePreparedAttempt,
        _: ModelRouteSendPermit,
    ) -> Result<Vec<u8>> {
        Err(Error::TransportUnavailable)
    }
}

/// Implementations must commit `begin_send` before exposing a permit to transport,
/// seal raw bytes before parsing, and recheck currentness on every transition.
#[async_trait]
pub trait ModelRouteAttemptStore: Send {
    /// Trusted owner-approval verification seam. Stored signature syntax alone
    /// never authorizes a send.
    async fn authorized_budget_policy(
        &mut self,
        _workspace_id: Uuid,
        _now_unix_ms: i64,
    ) -> Result<Option<AdvisoryBudgetPolicy>> {
        Ok(None)
    }
    async fn by_preparation(
        &mut self,
        workspace_id: Uuid,
        preparation_request_key: &str,
        invocation: ModelRouteInvocation,
    ) -> Result<Option<ModelRouteAttemptSnapshot>>;
    async fn recover_raw_sealed(
        &mut self,
        _workspace_id: Uuid,
        _preparation_request_key: &str,
        _invocation: ModelRouteInvocation,
    ) -> Result<Option<(ModelRoutePreparedAttempt, ModelRouteSendPermit)>> {
        Ok(None)
    }
    async fn record_no_call(
        &mut self,
        prepared: &PreparedModelRouteRecommendation,
        invocation: ModelRouteInvocation,
        reason: ModelRouteRunNoCall,
    ) -> Result<()>;
    async fn begin_send(
        &mut self,
        prepared: &PreparedModelRouteRecommendation,
        invocation: ModelRouteInvocation,
        attempted: &ModelRoutePreparedAttempt,
        policy: &AdvisoryBudgetPolicy,
    ) -> Result<Option<ModelRouteSendPermit>>;
    async fn seal_raw_response(
        &mut self,
        permit: &ModelRouteSendPermit,
        raw: &[u8],
        response_sha256: &str,
    ) -> Result<()>;
    async fn sealed_response(&mut self, permit: &ModelRouteSendPermit) -> Result<Option<Vec<u8>>>;
    /// Returns true for missing, unknown, or overrun usage. Exact replay is idempotent.
    async fn consume_budget(
        &mut self,
        _permit: &ModelRouteSendPermit,
        _observation: &ModelRouteProviderObservation,
    ) -> Result<bool> {
        Err(Error::Forbidden)
    }
    async fn consumption_healthy(
        &mut self,
        _permit: &ModelRouteSendPermit,
    ) -> Result<Option<bool>> {
        Err(Error::Forbidden)
    }
    async fn capture_sealed_outcome(
        &mut self,
        evidence: &ModelRouteSealedRankingEvidence,
    ) -> Result<()>;
    async fn mark_send_unknown(&mut self, permit: &ModelRouteSendPermit) -> Result<()>;
}
