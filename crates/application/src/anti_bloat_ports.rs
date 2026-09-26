//! Durable, default-deny boundaries for a source-relative review of one saved graph.
use async_trait::async_trait;
use tect_domain::{
    AdvisoryBudgetPolicy, AntiBloatApplyReceipt, AntiBloatDisposition, AntiBloatInput,
    AntiBloatPreservation, AntiBloatPreservationAttestation, AntiBloatReview, CandidateDeltaBatch,
    ResolvedCandidateDraft, Result, WorkspaceAdvisoryMode,
};
use uuid::Uuid;

pub use tect_domain::AntiBloatVerificationMaterial;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AntiBloatNoCall {
    Disabled,
    Skipped,
    NoEligibleFindings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AntiBloatAttemptState {
    NoCall(AntiBloatNoCall),
    Prepared,
    Sending,
    Ranked(Vec<String>),
    SendUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAntiBloatReview {
    pub review_id: Uuid,
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    pub input: AntiBloatInput,
    pub review: AntiBloatReview,
    pub state: AntiBloatAttemptState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntiBloatPreparedRequest {
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub material_sha256: String,
    pub adapter_identity: String,
}

/// Typed frozen material given to a provider before the durable send fence.
pub struct AntiBloatRankingMaterial<'a> {
    pub saved: &'a StoredAntiBloatReview,
    pub eligible_ids: &'a [String],
}

pub fn anti_bloat_material_sha256(saved: &StoredAntiBloatReview) -> Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(&(
        saved.review_id,
        saved.workspace_id,
        saved.actor_id,
        &saved.input,
        &saved.review,
    ))
    .map_err(|_| tect_domain::Error::InternalInvariant)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntiBloatSendPermit {
    pub review_id: Uuid,
    pub request: AntiBloatPreparedRequest,
}

/// Provider supplied usage is evidence, never a budget estimate. Missing values
/// exhaust the attempt and suppress its ranking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntiBloatProviderObservation {
    pub raw: Vec<u8>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub elapsed_monotonic_ms: Option<i64>,
}

impl AntiBloatProviderObservation {
    pub fn normalized(mut self) -> Self {
        self.input_tokens = self.input_tokens.filter(|value| *value >= 0);
        self.output_tokens = self.output_tokens.filter(|value| *value >= 0);
        self.elapsed_monotonic_ms = self.elapsed_monotonic_ms.filter(|value| *value >= 0);
        self
    }
}

/// A database adapter must bind every operation to the authorized actor and
/// workspace. `begin_send` atomically rechecks the frozen source/plan revision,
/// stores the exact prepared bytes and digest, and commits the one-use fence
/// before returning a permit. A replay or uncertain send returns None.
#[async_trait]
pub trait AntiBloatStore: Send {
    /// Must verify owner approval using a trusted key. The default denies send.
    async fn authorized_budget_policy(
        &mut self,
        _workspace_id: Uuid,
        _now_unix_ms: i64,
    ) -> Result<Option<AdvisoryBudgetPolicy>> {
        Ok(None)
    }
    async fn advisory_mode(&mut self, workspace_id: Uuid) -> Result<WorkspaceAdvisoryMode>;

    async fn authoritative_input(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        expected_revision: i64,
    ) -> Result<Option<AntiBloatInput>>;

    async fn save_review(&mut self, record: StoredAntiBloatReview)
    -> Result<StoredAntiBloatReview>;

    async fn review(&mut self, review_id: Uuid) -> Result<Option<StoredAntiBloatReview>>;

    async fn begin_send(
        &mut self,
        saved: &StoredAntiBloatReview,
        prepared: &AntiBloatPreparedRequest,
        policy: &AdvisoryBudgetPolicy,
    ) -> Result<Option<AntiBloatSendPermit>>;

    async fn mark_send_unknown(&mut self, review_id: Uuid) -> Result<()>;

    /// Durably seals the unmodified transport response before interpretation.
    async fn seal_response(
        &mut self,
        permit: &AntiBloatSendPermit,
        raw_response: &[u8],
        response_sha256: &str,
    ) -> Result<()>;

    /// Called only after a separate transaction has committed the raw seal.
    /// Exact replay returns the same exhausted decision without a second charge.
    async fn consume_budget(
        &mut self,
        permit: &AntiBloatSendPermit,
        observation: &AntiBloatProviderObservation,
    ) -> Result<bool>;

    /// Returns only exact raw bytes with committed seal and successful committed
    /// budget consumption for this permit. Default denies interpretation.
    async fn authorized_sealed_response(
        &mut self,
        _permit: &AntiBloatSendPermit,
    ) -> Result<Vec<u8>> {
        Err(tect_domain::Error::InputConflict)
    }

    async fn seal_ranked(&mut self, review_id: Uuid, ranked_ids: &[String]) -> Result<()>;

    /// This seam must atomically save explicit disposition and preservation
    /// with a new authoritative draft at the next candidate-set revision. It
    /// must recheck `input` against source of truth in the same transaction as
    /// the native draft CAS; it must never synthesize operations from a finding
    /// or provider response. Exact replay returns the original native receipt.
    async fn apply_preserved_delta(
        &mut self,
        authored: &AntiBloatAuthoredDelta,
        input: &AntiBloatInput,
        preservation: &AntiBloatPreservation,
        after: &ResolvedCandidateDraft,
    ) -> Result<AntiBloatApplyReceipt>;
}

/// Transport receives only the exact durably prepared request bytes. Implementations cannot
/// obtain an attempt through the application path for disabled/skip/no-eligible.
#[async_trait]
pub trait AntiBloatRankingProvider: Send + Sync {
    fn adapter_identity(&self) -> &'static str {
        "generic-json-v1"
    }

    /// Pure wire preparation; called before reservation and fence commit.
    fn prepare(&self, material: &AntiBloatRankingMaterial<'_>) -> Result<Vec<u8>> {
        serde_json::to_vec(&serde_json::json!({
            "review": &material.saved.review, "eligible_ids": material.eligible_ids
        }))
        .map_err(|_| tect_domain::Error::InternalInvariant)
    }

    /// Pure interpretation of untouched transport bytes authorized by the store.
    fn parse_sealed(&self, raw: &[u8]) -> Result<Vec<String>> {
        serde_json::from_slice(raw).map_err(|_| tect_domain::Error::InputConflict)
    }
    async fn rank(&self, permit: &AntiBloatSendPermit) -> Result<AntiBloatProviderObservation>;
}

#[derive(Debug, Default)]
pub struct DisabledAntiBloatRankingProvider;

#[async_trait]
impl AntiBloatRankingProvider for DisabledAntiBloatRankingProvider {
    async fn rank(&self, _: &AntiBloatSendPermit) -> Result<AntiBloatProviderObservation> {
        Err(tect_domain::Error::Forbidden)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntiBloatAuthoredDelta {
    pub review_id: Uuid,
    pub finding_id: String,
    pub disposition: AntiBloatDisposition,
    pub delta: CandidateDeltaBatch,
}

#[async_trait]
pub trait AntiBloatVerificationStore: Send {
    async fn anti_bloat_verification_material(
        &mut self,
        workspace_id: Uuid,
        review_id: Uuid,
        lock: bool,
    ) -> Result<Option<AntiBloatVerificationMaterial>>;

    async fn anti_bloat_attestation_by_request(
        &mut self,
        workspace_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<AntiBloatPreservationAttestation>>;

    async fn append_anti_bloat_attestation(
        &mut self,
        value: &AntiBloatPreservationAttestation,
    ) -> Result<()>;
}
