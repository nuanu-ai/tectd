//! Typed, additive mutations for the normalized scope-candidate graph.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const MAX_DELTA_OPERATIONS: usize = 100;
pub const MAX_IDEMPOTENCY_KEY_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateDeltaTargetKind {
    Goal,
    Candidate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalDeltaValue {
    pub text: String,
    pub finite: bool,
    pub source_ref_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateDeltaValue {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceDeltaValue {
    pub summary: String,
    pub source_ref_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockerDeltaValue {
    pub summary: String,
    pub source_ref_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", deny_unknown_fields)]
pub enum CandidateDeltaOperation {
    #[serde(rename = "goal.add")]
    GoalAdd {
        goal_id: Uuid,
        #[serde(flatten)]
        value: GoalDeltaValue,
    },
    #[serde(rename = "goal.resolve")]
    GoalResolve {
        goal_id: Uuid,
        expected_revision: i64,
    },
    #[serde(rename = "candidate.add")]
    CandidateAdd {
        candidate_id: Uuid,
        #[serde(flatten)]
        value: CandidateDeltaValue,
    },
    #[serde(rename = "candidate.update")]
    CandidateUpdate {
        candidate_id: Uuid,
        expected_revision: i64,
        #[serde(flatten)]
        value: CandidateDeltaValue,
    },
    #[serde(rename = "candidate.remove")]
    CandidateRemove {
        candidate_id: Uuid,
        expected_revision: i64,
    },
    #[serde(rename = "candidate.supersede")]
    CandidateSupersede {
        candidate_id: Uuid,
        replacement_candidate_id: Uuid,
        expected_revision: i64,
    },
    #[serde(rename = "coverage.link")]
    CoverageLink { candidate_id: Uuid, goal_id: Uuid },
    #[serde(rename = "coverage.unlink")]
    CoverageUnlink { candidate_id: Uuid, goal_id: Uuid },
    #[serde(rename = "evidence.add")]
    EvidenceAdd {
        evidence_id: Uuid,
        target_kind: CandidateDeltaTargetKind,
        target_id: Uuid,
        #[serde(flatten)]
        value: EvidenceDeltaValue,
    },
    #[serde(rename = "evidence.update")]
    EvidenceUpdate {
        evidence_id: Uuid,
        expected_revision: i64,
        #[serde(flatten)]
        value: EvidenceDeltaValue,
    },
    #[serde(rename = "evidence.remove")]
    EvidenceRemove {
        evidence_id: Uuid,
        expected_revision: i64,
    },
    #[serde(rename = "blocker.add")]
    BlockerAdd {
        blocker_id: Uuid,
        goal_id: Uuid,
        #[serde(flatten)]
        value: BlockerDeltaValue,
    },
    #[serde(rename = "blocker.update")]
    BlockerUpdate {
        blocker_id: Uuid,
        expected_revision: i64,
        #[serde(flatten)]
        value: BlockerDeltaValue,
    },
    #[serde(rename = "blocker.remove")]
    BlockerRemove {
        blocker_id: Uuid,
        expected_revision: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateDeltaBatch {
    pub candidate_set_id: Uuid,
    pub expected_revision: i64,
    pub idempotency_key: String,
    pub operations: Vec<CandidateDeltaOperation>,
}

impl CandidateDeltaBatch {
    pub fn validate(&self) -> Result<()> {
        if self.candidate_set_id.is_nil()
            || self.expected_revision < 1
            || self.idempotency_key.is_empty()
            || self.idempotency_key.len() > MAX_IDEMPOTENCY_KEY_BYTES
            || self.operations.is_empty()
            || self.operations.len() > MAX_DELTA_OPERATIONS
            || !self
                .idempotency_key
                .bytes()
                .all(|byte| byte.is_ascii_graphic())
        {
            return Err(Error::InvalidArguments);
        }
        let mut creates = BTreeSet::new();
        for operation in &self.operations {
            operation.validate(&mut creates)?;
        }
        Ok(())
    }
}

impl CandidateDeltaOperation {
    fn validate(&self, creates: &mut BTreeSet<Uuid>) -> Result<()> {
        use CandidateDeltaOperation as Op;
        let valid_text = |value: &str| !value.trim().is_empty() && !value.contains('\0');
        match self {
            Op::GoalAdd { goal_id, value } => {
                require_create(creates, *goal_id)?;
                if !valid_text(&value.text) || value.source_ref_id.is_nil() {
                    return Err(Error::InvalidArguments);
                }
            }
            Op::CandidateAdd {
                candidate_id,
                value,
            } => {
                require_create(creates, *candidate_id)?;
                if !valid_text(&value.title)
                    || value
                        .outcome
                        .as_deref()
                        .is_some_and(|text| !valid_text(text))
                {
                    return Err(Error::InvalidArguments);
                }
            }
            Op::EvidenceAdd {
                evidence_id,
                target_id,
                value,
                ..
            } => {
                require_create(creates, *evidence_id)?;
                if target_id.is_nil() || value.source_ref_id.is_nil() || !valid_text(&value.summary)
                {
                    return Err(Error::InvalidArguments);
                }
            }
            Op::BlockerAdd {
                blocker_id,
                goal_id,
                value,
            } => {
                require_create(creates, *blocker_id)?;
                if goal_id.is_nil() || value.source_ref_id.is_nil() || !valid_text(&value.summary) {
                    return Err(Error::InvalidArguments);
                }
            }
            Op::GoalResolve {
                goal_id,
                expected_revision,
            }
            | Op::CandidateRemove {
                candidate_id: goal_id,
                expected_revision,
            }
            | Op::EvidenceRemove {
                evidence_id: goal_id,
                expected_revision,
            }
            | Op::BlockerRemove {
                blocker_id: goal_id,
                expected_revision,
            } if goal_id.is_nil() || *expected_revision < 1 => {
                return Err(Error::InvalidArguments);
            }
            Op::CandidateUpdate {
                candidate_id,
                expected_revision,
                value,
            } => {
                if candidate_id.is_nil()
                    || *expected_revision < 1
                    || !valid_text(&value.title)
                    || value
                        .outcome
                        .as_deref()
                        .is_some_and(|text| !valid_text(text))
                {
                    return Err(Error::InvalidArguments);
                }
            }
            Op::CandidateSupersede {
                candidate_id,
                replacement_candidate_id,
                expected_revision,
            } => {
                if candidate_id.is_nil()
                    || replacement_candidate_id.is_nil()
                    || candidate_id == replacement_candidate_id
                    || *expected_revision < 1
                {
                    return Err(Error::InvalidArguments);
                }
            }
            Op::CoverageLink {
                candidate_id,
                goal_id,
            }
            | Op::CoverageUnlink {
                candidate_id,
                goal_id,
            } if candidate_id.is_nil() || goal_id.is_nil() => {
                return Err(Error::InvalidArguments);
            }
            Op::EvidenceUpdate {
                evidence_id,
                expected_revision,
                value,
            } => {
                if evidence_id.is_nil()
                    || *expected_revision < 1
                    || value.source_ref_id.is_nil()
                    || !valid_text(&value.summary)
                {
                    return Err(Error::InvalidArguments);
                }
            }
            Op::BlockerUpdate {
                blocker_id,
                expected_revision,
                value,
            } => {
                if blocker_id.is_nil()
                    || *expected_revision < 1
                    || value.source_ref_id.is_nil()
                    || !valid_text(&value.summary)
                {
                    return Err(Error::InvalidArguments);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn require_create(creates: &mut BTreeSet<Uuid>, id: Uuid) -> Result<()> {
    if id.is_nil() || !creates.insert(id) {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateDeltaReceipt {
    pub candidate_set_id: Uuid,
    pub idempotency_key: String,
    pub from_revision: i64,
    pub to_revision: i64,
    pub stale_reasons: Vec<String>,
}

pub fn validate_replay(
    stored: Option<&CandidateDeltaBatch>,
    request: &CandidateDeltaBatch,
) -> Result<bool> {
    request.validate()?;
    match stored {
        None => Ok(false),
        Some(previous) if previous == request => Ok(true),
        Some(_) => Err(Error::InputConflict),
    }
}

pub fn stale_reasons(operations: &[CandidateDeltaOperation]) -> Vec<String> {
    let mut reasons = BTreeSet::new();
    for operation in operations {
        use CandidateDeltaOperation as Op;
        match operation {
            Op::GoalAdd { .. } | Op::GoalResolve { .. } => {
                reasons.insert("goals".to_owned());
            }
            Op::CandidateAdd { .. }
            | Op::CandidateUpdate { .. }
            | Op::CandidateRemove { .. }
            | Op::CandidateSupersede { .. } => {
                reasons.insert("candidates".to_owned());
            }
            Op::CoverageLink { .. } | Op::CoverageUnlink { .. } => {
                reasons.insert("coverage".to_owned());
            }
            Op::EvidenceAdd { .. } | Op::EvidenceUpdate { .. } | Op::EvidenceRemove { .. } => {
                reasons.insert("evidence".to_owned());
            }
            Op::BlockerAdd { .. } | Op::BlockerUpdate { .. } | Op::BlockerRemove { .. } => {
                reasons.insert("blockers".to_owned());
            }
        }
    }
    reasons.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: u128) -> CandidateDeltaOperation {
        CandidateDeltaOperation::CandidateAdd {
            candidate_id: Uuid::from_u128(id),
            value: CandidateDeltaValue {
                title: format!("candidate-{id}"),
                outcome: None,
            },
        }
    }

    fn batch(key: &str, operations: Vec<CandidateDeltaOperation>) -> CandidateDeltaBatch {
        CandidateDeltaBatch {
            candidate_set_id: Uuid::from_u128(1),
            expected_revision: 2,
            idempotency_key: key.into(),
            operations,
        }
    }

    #[test]
    fn exact_operations_round_trip_and_duplicate_creates_fail() {
        let value = batch("wire-1", vec![candidate(2)]);
        let encoded = serde_json::to_value(&value).unwrap();
        assert_eq!(encoded["operations"][0]["operation"], "candidate.add");
        assert_eq!(
            serde_json::from_value::<CandidateDeltaBatch>(encoded).unwrap(),
            value
        );
        assert_eq!(
            batch("duplicate", vec![candidate(2), candidate(2)]).validate(),
            Err(Error::InvalidArguments)
        );
    }

    #[test]
    fn self_supersession_is_rejected() {
        let id = Uuid::from_u128(2);
        assert_eq!(
            batch(
                "self",
                vec![CandidateDeltaOperation::CandidateSupersede {
                    candidate_id: id,
                    replacement_candidate_id: id,
                    expected_revision: 1,
                }]
            )
            .validate(),
            Err(Error::InvalidArguments)
        );
    }

    #[test]
    fn exact_replay_is_accepted_and_key_conflict_rejected() {
        let value = batch("k", vec![candidate(2)]);
        assert_eq!(validate_replay(Some(&value), &value), Ok(true));
        let changed = batch("k", vec![candidate(3)]);
        assert_eq!(
            validate_replay(Some(&value), &changed),
            Err(Error::InputConflict)
        );
    }
}
