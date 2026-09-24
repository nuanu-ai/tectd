use async_trait::async_trait;
use tect_domain::{Error, MatrixAdviceEligibility, MatrixRanking, Result};
use uuid::Uuid;

use crate::MatrixProviderBinding;

/// Durable evidence of one guarded Matrix dispatch. A ranking is advisory only;
/// selection belongs to the later, separate disposition record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardedMatrixAdviceRecord {
    pub opportunity_id: Uuid,
    pub dispatch_id: Uuid,
    pub opportunity_material_digest: String,
    pub binding: MatrixProviderBinding,
    pub outcome: GuardedMatrixAdviceOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardedMatrixAdviceOutcome {
    Ranked {
        ranked_choice_ids: Vec<String>,
    },
    Abstained {
        reason: Option<String>,
    },
    /// A rejected response yields no usable advice and no ranking.
    Rejected {
        reason: String,
    },
}

impl GuardedMatrixAdviceRecord {
    /// Validate against the fresh opportunity, dispatch and saved Matrix
    /// revision/evaluation binding that the store must lock and read.
    pub fn validate_for(
        &self,
        opportunity_id: Uuid,
        dispatch_id: Uuid,
        binding: &MatrixProviderBinding,
        eligibility: &MatrixAdviceEligibility,
    ) -> Result<()> {
        if self.opportunity_id.is_nil()
            || self.dispatch_id.is_nil()
            || self.opportunity_id != opportunity_id
            || self.dispatch_id != dispatch_id
            || self.binding != *binding
            || self.binding.task_id.is_nil()
            || self.binding.task_revision < 1
            || self.opportunity_material_digest != binding.evaluation_digest
            || !is_digest(&binding.input_digest)
            || !is_digest(&binding.choice_set_digest)
            || !is_digest(&binding.evaluation_digest)
        {
            return Err(Error::InvalidArguments);
        }
        match &self.outcome {
            GuardedMatrixAdviceOutcome::Ranked { ranked_choice_ids } => {
                let ranking = MatrixRanking::Ranked {
                    ranked_candidate_ids: ranked_choice_ids.clone(),
                    recommended_candidate_id: ranked_choice_ids
                        .first()
                        .cloned()
                        .ok_or(Error::InvalidArguments)?,
                };
                ranking.validate(eligibility)
            }
            GuardedMatrixAdviceOutcome::Abstained { reason } => {
                valid_reason(reason.as_deref())?;
                MatrixRanking::Abstained {
                    ranked_candidate_ids: Vec::new(),
                    recommended_candidate_id: None,
                }
                .validate(eligibility)
            }
            GuardedMatrixAdviceOutcome::Rejected { reason } => {
                valid_reason(Some(reason))?;
                if !matches!(
                    eligibility,
                    MatrixAdviceEligibility::EligibleForAdvice { .. }
                ) {
                    return Err(Error::InvalidArguments);
                }
                Ok(())
            }
        }
    }
}

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_reason(reason: Option<&str>) -> Result<()> {
    if reason.is_some_and(|value| value.trim().is_empty()) {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredGuardedMatrixAdviceRecord {
    pub advice_id: Uuid,
    pub record: GuardedMatrixAdviceRecord,
}

/// The adapter must lock the opportunity and dispatch, check their lifecycle
/// state and exact saved revision/evaluation, validate the outcome against the
/// owner choice set, then insert once. It must not derive a disposition here.
#[async_trait]
pub trait MatrixAdviceStore: Send {
    async fn guarded_matrix_advice(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<Option<StoredGuardedMatrixAdviceRecord>>;

    async fn persist_guarded_matrix_advice(
        &mut self,
        workspace_id: Uuid,
        record: &GuardedMatrixAdviceRecord,
    ) -> Result<StoredGuardedMatrixAdviceRecord>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> MatrixProviderBinding {
        MatrixProviderBinding {
            task_id: Uuid::new_v4(),
            task_revision: 2,
            input_digest: "a".repeat(64),
            choice_set_id: "set-1".into(),
            choice_set_version: 1,
            choice_set_digest: "b".repeat(64),
            evaluation_digest: "c".repeat(64),
        }
    }

    fn record(outcome: GuardedMatrixAdviceOutcome) -> GuardedMatrixAdviceRecord {
        let binding = binding();
        GuardedMatrixAdviceRecord {
            opportunity_id: Uuid::new_v4(),
            dispatch_id: Uuid::new_v4(),
            opportunity_material_digest: binding.evaluation_digest.clone(),
            binding,
            outcome,
        }
    }

    fn eligibility() -> MatrixAdviceEligibility {
        MatrixAdviceEligibility::EligibleForAdvice {
            candidate_ids: vec!["a".into(), "b".into()],
        }
    }

    fn validate(record: &GuardedMatrixAdviceRecord) -> Result<()> {
        record.validate_for(
            record.opportunity_id,
            record.dispatch_id,
            &record.binding,
            &eligibility(),
        )
    }

    #[test]
    fn ranked_requires_exact_permutation() {
        let mut record = record(GuardedMatrixAdviceOutcome::Ranked {
            ranked_choice_ids: vec!["b".into(), "a".into()],
        });
        assert_eq!(validate(&record), Ok(()));
        record.outcome = GuardedMatrixAdviceOutcome::Ranked {
            ranked_choice_ids: vec!["a".into(), "a".into()],
        };
        assert_eq!(validate(&record), Err(Error::InvalidArguments));
    }

    #[test]
    fn abstention_is_usable_without_a_rank() {
        let record = record(GuardedMatrixAdviceOutcome::Abstained { reason: None });
        assert_eq!(validate(&record), Ok(()));
    }

    #[test]
    fn rejection_has_no_usable_rank_and_requires_reason() {
        let mut record = record(GuardedMatrixAdviceOutcome::Rejected {
            reason: "invalid provider response".into(),
        });
        assert_eq!(validate(&record), Ok(()));
        record.outcome = GuardedMatrixAdviceOutcome::Rejected { reason: " ".into() };
        assert_eq!(validate(&record), Err(Error::InvalidArguments));
    }

    #[test]
    fn rejects_mismatched_binding_and_opportunity_material() {
        let mut record = record(GuardedMatrixAdviceOutcome::Abstained { reason: None });
        let expected = record.binding.clone();
        record.binding.task_revision += 1;
        assert_eq!(
            record.validate_for(
                record.opportunity_id,
                record.dispatch_id,
                &expected,
                &eligibility()
            ),
            Err(Error::InvalidArguments)
        );
        record.binding = expected;
        record.opportunity_material_digest = "d".repeat(64);
        assert_eq!(validate(&record), Err(Error::InvalidArguments));
    }
}
