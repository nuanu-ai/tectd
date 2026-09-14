use crate::{
    DK2_MAX_OPERATIONS, Error, KnowledgeCompletionRequirement, KnowledgeErasureRequirement,
    KnowledgeLifecycleState, KnowledgeSearchRequirement, Result,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeErasedNoChangeOperationProof {
    pub operation_id: Uuid,
    pub unit_id: Uuid,
    pub expected_revision: i64,
    pub expected_lifecycle: KnowledgeLifecycleState,
    pub erasure_sequence: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeErasedNoChangeProof {
    pub completion: KnowledgeCompletionRequirement,
    pub operations: Vec<KnowledgeErasedNoChangeOperationProof>,
}

impl KnowledgeErasedNoChangeProof {
    pub fn validate(&self) -> Result<()> {
        if self.operations.is_empty()
            || self.operations.len() > DK2_MAX_OPERATIONS
            || self.completion.search != KnowledgeSearchRequirement::NotRequired
            || matches!(
                self.completion.erasure,
                KnowledgeErasureRequirement::NotRequired
                    | KnowledgeErasureRequirement::AllRetainedCopies
            )
            || self.operations.iter().any(|operation| {
                operation.operation_id.is_nil()
                    || operation.unit_id.is_nil()
                    || operation.expected_revision < 1
                    || operation.expected_lifecycle != KnowledgeLifecycleState::Erased
                    || operation.erasure_sequence < 1
            })
            || self
                .operations
                .iter()
                .map(|operation| operation.operation_id)
                .collect::<BTreeSet<_>>()
                .len()
                != self.operations.len()
            || self
                .operations
                .iter()
                .map(|operation| operation.unit_id)
                .collect::<BTreeSet<_>>()
                .len()
                != self.operations.len()
        {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proof() -> KnowledgeErasedNoChangeProof {
        KnowledgeErasedNoChangeProof {
            completion: KnowledgeCompletionRequirement {
                canonical_result: true,
                exact_delivery: true,
                impact_recorded: true,
                search: KnowledgeSearchRequirement::NotRequired,
                erasure: KnowledgeErasureRequirement::OwnedLiveCopies,
            },
            operations: vec![KnowledgeErasedNoChangeOperationProof {
                operation_id: Uuid::new_v4(),
                unit_id: Uuid::new_v4(),
                expected_revision: 1,
                expected_lifecycle: KnowledgeLifecycleState::Erased,
                erasure_sequence: 1,
            }],
        }
    }

    #[test]
    fn opaque_proof_accepts_only_bounded_completed_suppression() {
        assert_eq!(proof().validate(), Ok(()));
        let mut duplicate = proof();
        duplicate.operations.push(duplicate.operations[0].clone());
        assert_eq!(duplicate.validate(), Err(Error::InvalidArguments));
        let mut unsupported = proof();
        unsupported.completion.erasure = KnowledgeErasureRequirement::AllRetainedCopies;
        assert_eq!(unsupported.validate(), Err(Error::InvalidArguments));
        let mut wrong_state = proof();
        wrong_state.operations[0].expected_lifecycle = KnowledgeLifecycleState::Active;
        assert_eq!(wrong_state.validate(), Err(Error::InvalidArguments));
    }
}
