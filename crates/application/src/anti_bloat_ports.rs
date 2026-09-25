//! Durable, default-deny boundaries for a source-relative review of one saved graph.
use async_trait::async_trait;
use tect_domain::{
    AntiBloatApplyReceipt, AntiBloatDisposition, AntiBloatInput, AntiBloatPreservation,
    AntiBloatPreservationAttestation, AntiBloatReview, CandidateDeltaBatch, ResolvedCandidateDraft,
    Result, WorkspaceAdvisoryMode,
};
use uuid::Uuid;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntiBloatSendPermit {
    pub review_id: Uuid,
    pub request: AntiBloatPreparedRequest,
}

/// A database adapter must bind every operation to the authorized actor and
/// workspace. `begin_send` atomically rechecks the frozen source/plan revision,
/// stores the exact prepared bytes and digest, and commits the one-use fence
/// before returning a permit. A replay or uncertain send returns None.
#[async_trait]
pub trait AntiBloatStore: Send {
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
    ) -> Result<Option<AntiBloatSendPermit>>;

    async fn mark_send_unknown(&mut self, review_id: Uuid) -> Result<()>;

    /// Durably seals the unmodified transport response before interpretation.
    async fn seal_response(
        &mut self,
        permit: &AntiBloatSendPermit,
        raw_response: &[u8],
        response_sha256: &str,
    ) -> Result<()>;

    async fn seal_ranked(&mut self, review_id: Uuid, ranked_ids: &[String]) -> Result<()>;

    /// This seam must atomically save explicit disposition and preservation
    /// with a new authoritative draft at the next candidate-set revision. It
    /// must recheck `input` against source of truth in the same transaction as
    /// the native draft CAS; it must never synthesize operations from a finding
    /// or provider response. Exact replay returns the original native receipt.
    async fn apply_preserved_delta(
        &mut self,
        review_id: Uuid,
        input: &AntiBloatInput,
        finding_id: &str,
        disposition: AntiBloatDisposition,
        preservation: &AntiBloatPreservation,
        delta: &CandidateDeltaBatch,
        after: &ResolvedCandidateDraft,
    ) -> Result<AntiBloatApplyReceipt>;
}

/// Transport receives only the exact durably prepared request bytes. Implementations cannot
/// obtain an attempt through the application path for disabled/skip/no-eligible.
#[async_trait]
pub trait AntiBloatRankingProvider: Send + Sync {
    async fn rank(&self, permit: &AntiBloatSendPermit) -> Result<Vec<u8>>;
}

#[derive(Debug, Default)]
pub struct DisabledAntiBloatRankingProvider;

#[async_trait]
impl AntiBloatRankingProvider for DisabledAntiBloatRankingProvider {
    async fn rank(&self, _: &AntiBloatSendPermit) -> Result<Vec<u8>> {
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

/// All evidence is re-read from the immutable caller/review ledger and native
/// saved drafts. The caller cannot supply any of these facts.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AntiBloatVerificationMaterial {
    pub workspace_id: Uuid,
    pub review_id: Uuid,
    pub review_actor_id: Uuid,
    pub selected_disposition_actor_id: Uuid,
    pub selected_caller_actor_id: Uuid,
    pub selected_caller_session_id: Uuid,
    pub input: AntiBloatInput,
    pub review: AntiBloatReview,
    pub finding_id: String,
    pub disposition: AntiBloatDisposition,
    pub preservation: AntiBloatPreservation,
    pub delta: CandidateDeltaBatch,
    pub claimed_after: ResolvedCandidateDraft,
    pub receipt: AntiBloatApplyReceipt,
    pub before_saved: ResolvedCandidateDraft,
    pub after_saved: ResolvedCandidateDraft,
    pub current_revision: i64,
    pub source_fragments_match: bool,
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
