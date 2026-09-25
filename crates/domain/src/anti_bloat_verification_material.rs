//! Immutable evidence and request shapes for independent anti-bloat verification.
use crate::{
    AntiBloatApplyReceipt, AntiBloatDisposition, AntiBloatInput, AntiBloatPreservation,
    AntiBloatReview, CandidateDeltaBatch, ResolvedCandidateDraft,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// All evidence is re-read from the immutable caller/review ledger and native
/// saved drafts. The caller cannot supply any of these facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyAntiBloatApply {
    pub request_id: Uuid,
    pub review_id: Uuid,
    pub expected_evidence_digest: String,
}
