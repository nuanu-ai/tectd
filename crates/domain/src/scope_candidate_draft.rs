use crate::{CandidateBoundary, Error, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
}

impl DraftIdentity {
    pub fn validate(&self) -> Result<()> {
        match (&self.local, self.id, self.revision) {
            (Some(local), None, None)
                if !local.is_empty()
                    && local.len() <= 64
                    && local
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)) =>
            {
                Ok(())
            }
            (None, Some(id), Some(revision)) if !id.is_nil() && revision >= 1 => Ok(()),
            _ => Err(Error::InvalidArguments),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
}

impl CandidateRef {
    pub fn validate(&self) -> Result<()> {
        match (&self.local, self.id) {
            (Some(local), None) if !local.is_empty() && local.len() <= 64 => Ok(()),
            (None, Some(id)) if !id.is_nil() => Ok(()),
            _ => Err(Error::InvalidArguments),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageGoalDraft {
    pub identity: DraftIdentity,
    pub text: String,
    pub source_ref_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_quote: Option<String>,
    pub resolution: CoverageResolutionDraft,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageResolutionKind {
    Candidate,
    Evidence,
    Blocker,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageResolutionDraft {
    pub kind: CoverageResolutionKind,
    pub reference: CandidateRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    VerifiedEvidence,
    AcceptedWork,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceDraft {
    pub identity: DraftIdentity,
    pub kind: EvidenceKind,
    pub summary: String,
    pub source_ref_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_input_sequence: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateDraft {
    pub identity: DraftIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_rationale: Option<String>,
    pub title: String,
    pub outcome: String,
    pub trigger: String,
    pub delivered_behavior: String,
    pub proof: String,
    #[serde(default)]
    pub includes: Vec<String>,
    #[serde(default)]
    pub excludes: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<CandidateRef>,
    pub coverage_goals: Vec<CandidateRef>,
    #[serde(default)]
    pub evidence: Vec<CandidateRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockerDraft {
    pub identity: DraftIdentity,
    pub summary: String,
    pub source_ref_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeCandidateDraft {
    pub boundary: CandidateBoundary,
    pub goals: Vec<CoverageGoalDraft>,
    #[serde(default)]
    pub evidence: Vec<EvidenceDraft>,
    pub candidates: Vec<CandidateDraft>,
    #[serde(default)]
    pub blockers: Vec<BlockerDraft>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_question: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty_disposition: Option<EmptyCandidateDisposition>,
    #[serde(default)]
    pub protected_changes: Vec<ProtectedChangeDraft>,
    #[serde(default)]
    pub supersessions: Vec<CandidateSupersessionDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateSupersessionDraft {
    pub candidate_id: Uuid,
    pub revision: i64,
    pub reason: String,
    #[serde(default)]
    pub replacements: Vec<CandidateRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtectedChangeDisposition {
    Delete,
    Replace,
    Reassociate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtectedChangeDraft {
    pub accepted_evidence_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_candidate_id: Option<Uuid>,
    pub disposition: ProtectedChangeDisposition,
    pub rationale: String,
    pub authority_source_ref_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement_evidence: Option<CandidateRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_candidate: Option<CandidateRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmptyCandidateDispositionKind {
    AllCovered,
    OutOfBoundary,
    NeedsInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyCandidateDisposition {
    pub kind: EmptyCandidateDispositionKind,
    pub reason: String,
    pub source_ref_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveCandidateDraft {
    pub candidate_set_id: Uuid,
    pub revision: i64,
    pub snapshot_id: Uuid,
    pub input_cursor: i64,
    pub request_id: Uuid,
    pub draft: ScopeCandidateDraft,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_knowledge: Option<crate::PlanningManifestGuard>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Ready,
    Revise,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateDecisionKind {
    Accept,
    Revise,
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateDecision {
    pub candidate_id: Uuid,
    pub decision: CandidateDecisionKind,
    pub rationale: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateFindingSeverity {
    Advisory,
    Material,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateFinding {
    pub severity: CandidateFindingSeverity,
    pub summary: String,
    #[serde(default)]
    pub candidate_ids: Vec<Uuid>,
    #[serde(default)]
    pub coverage_goal_ids: Vec<Uuid>,
    pub disposition: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeCandidateReview {
    pub revision: i64,
    pub verdict: ReviewVerdict,
    pub summary: String,
    pub findings: Vec<CandidateFinding>,
    pub candidate_decisions: Vec<CandidateDecision>,
    pub protected_change_reviews: Vec<ProtectedChangeReview>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateReviewDraft {
    pub verdict: ReviewVerdict,
    pub summary: String,
    pub findings: Vec<CandidateFinding>,
    pub candidate_decisions: Vec<CandidateDecision>,
    #[serde(default)]
    pub protected_change_reviews: Vec<ProtectedChangeReview>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtectedChangeReview {
    pub accepted_evidence_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_candidate_id: Option<Uuid>,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewCandidateSet {
    pub candidate_set_id: Uuid,
    pub revision: i64,
    pub snapshot_id: Uuid,
    pub input_cursor: i64,
    pub request_id: Uuid,
    pub review: CandidateReviewDraft,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_knowledge: Option<crate::PlanningManifestGuard>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageGoalEntity {
    pub id: Uuid,
    pub revision: i64,
    pub text: String,
    pub source_ref_id: Uuid,
    pub exact_quote: Option<String>,
    pub resolution: CoverageResolutionEntity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageResolutionEntity {
    pub kind: CoverageResolutionKind,
    pub id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceEntity {
    pub id: Uuid,
    pub revision: i64,
    pub kind: EvidenceKind,
    pub summary: String,
    pub source_ref_id: Uuid,
    pub authority_input_sequence: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateEntity {
    pub id: Uuid,
    pub revision: i64,
    pub title: String,
    pub outcome: String,
    pub trigger: String,
    pub delivered_behavior: String,
    pub proof: String,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    pub dependencies: Vec<Uuid>,
    pub coverage_goal_ids: Vec<Uuid>,
    pub evidence_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockerEntity {
    pub id: Uuid,
    pub revision: i64,
    pub summary: String,
    pub source_ref_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedCandidateDraft {
    pub boundary: CandidateBoundary,
    pub goals: Vec<CoverageGoalEntity>,
    pub evidence: Vec<EvidenceEntity>,
    pub candidates: Vec<CandidateEntity>,
    pub blockers: Vec<BlockerEntity>,
    pub pending_question: Option<String>,
    pub empty_disposition: Option<EmptyCandidateDisposition>,
    pub protected_changes: Vec<ProtectedChangeEntity>,
    #[serde(default)]
    pub delta: CandidateDelta,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateDelta {
    pub added: Vec<CandidateAdded>,
    pub changed: Vec<CandidateChanged>,
    pub unchanged: Vec<CandidateUnchanged>,
    pub superseded: Vec<CandidateSuperseded>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateAdded {
    pub candidate_id: Uuid,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateChanged {
    pub candidate_id: Uuid,
    pub from_revision: i64,
    pub to_revision: i64,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateUnchanged {
    pub candidate_id: Uuid,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSuperseded {
    pub prior: CandidateEntity,
    pub reason: String,
    pub replacement_candidate_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectedChangeEntity {
    pub accepted_evidence_id: Uuid,
    pub prior_candidate_id: Option<Uuid>,
    pub disposition: ProtectedChangeDisposition,
    pub rationale: String,
    pub authority_source_ref_id: Uuid,
    pub replacement_evidence_id: Option<Uuid>,
    pub target_candidate_id: Option<Uuid>,
}
