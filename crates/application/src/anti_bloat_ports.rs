//! Durable, default-deny boundaries for a source-relative review of one saved graph.
use async_trait::async_trait;
use tect_domain::{
    AntiBloatDisposition, AntiBloatInput, AntiBloatPreservation, AntiBloatReview,
    CandidateDeltaBatch, CandidateDeltaReceipt, ResolvedCandidateDraft, Result,
    WorkspaceAdvisoryMode,
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

/// A database adapter must bind every operation to the authorized actor and
/// workspace. `begin_send` atomically rechecks the frozen source/plan revision,
/// fences the review, and commits before returning true. A replay or uncertain
/// send always returns false.
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

    async fn begin_send(&mut self, saved: &StoredAntiBloatReview) -> Result<bool>;

    async fn mark_send_unknown(&mut self, review_id: Uuid) -> Result<()>;

    async fn seal_ranked(&mut self, review_id: Uuid, ranked_ids: &[String]) -> Result<()>;

    /// This seam must atomically save explicit disposition + preservation and
    /// invoke the existing candidate-delta caller with its CAS revision. It
    /// must recheck `input` against source of truth in the same transaction as
    /// the caller CAS; it must never synthesize operations from a finding or a
    /// provider response. A successful replay may return the original receipt.
    async fn apply_preserved_delta(
        &mut self,
        review_id: Uuid,
        input: &AntiBloatInput,
        finding_id: &str,
        disposition: AntiBloatDisposition,
        preservation: &AntiBloatPreservation,
        delta: &CandidateDeltaBatch,
    ) -> Result<CandidateDeltaReceipt>;
}

/// Transport receives only the frozen eligible set. Implementations cannot
/// obtain an attempt through the application path for disabled/skip/no-eligible.
#[async_trait]
pub trait AntiBloatRankingProvider: Send + Sync {
    async fn rank(&self, review: &AntiBloatReview, eligible_ids: &[String]) -> Result<Vec<String>>;
}

#[derive(Debug, Default)]
pub struct DisabledAntiBloatRankingProvider;

#[async_trait]
impl AntiBloatRankingProvider for DisabledAntiBloatRankingProvider {
    async fn rank(&self, _: &AntiBloatReview, _: &[String]) -> Result<Vec<String>> {
        Err(tect_domain::Error::Forbidden)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntiBloatAuthoredDelta {
    pub review_id: Uuid,
    pub finding_id: String,
    pub disposition: AntiBloatDisposition,
    pub delta: CandidateDeltaBatch,
    pub after: ResolvedCandidateDraft,
}
