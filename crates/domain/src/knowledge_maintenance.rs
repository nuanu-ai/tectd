use crate::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const KNOWLEDGE_MAINTENANCE_MAX_BATCH: u32 = 64;
pub const KNOWLEDGE_MAINTENANCE_MAX_ATTEMPTS: u32 = 5;
pub const KNOWLEDGE_MAINTENANCE_METHOD_ID: &str = "tect:knowledge-maintenance:method";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeMaintenanceSignalReason {
    ReviewDue,
    SourceChanged,
    DependencyChanged,
    ApplicationFailed,
    OperatorRequested,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum KnowledgeMaintenanceBasis {
    ReviewDue {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        review_due_at: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        valid_until: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        validation_event_id: Option<Uuid>,
    },
    SourceChanged {
        source_iri: String,
        accepted_digest: String,
        observed_digest: String,
    },
    DependencyChanged {
        dependency_unit_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous_event_id: Option<Uuid>,
        observed_event_id: Uuid,
    },
    ApplicationFailed {
        consumer_ref: String,
        failure_digest: String,
    },
    OperatorRequested {
        subject_ref: String,
        observation_digest: String,
    },
}

impl KnowledgeMaintenanceBasis {
    pub const fn reason(&self) -> KnowledgeMaintenanceSignalReason {
        match self {
            Self::ReviewDue { .. } => KnowledgeMaintenanceSignalReason::ReviewDue,
            Self::SourceChanged { .. } => KnowledgeMaintenanceSignalReason::SourceChanged,
            Self::DependencyChanged { .. } => KnowledgeMaintenanceSignalReason::DependencyChanged,
            Self::ApplicationFailed { .. } => KnowledgeMaintenanceSignalReason::ApplicationFailed,
            Self::OperatorRequested { .. } => KnowledgeMaintenanceSignalReason::OperatorRequested,
        }
    }

    pub fn validate(&self) -> Result<()> {
        let digest = |value: &str| bounded(value, 256);
        match self {
            Self::ReviewDue {
                review_due_at,
                valid_until,
                validation_event_id,
            } => {
                if review_due_at.is_none() && valid_until.is_none()
                    || review_due_at
                        .as_deref()
                        .is_some_and(|value| crate::knowledge_time::parse_rfc3339(value).is_none())
                    || valid_until
                        .as_deref()
                        .is_some_and(|value| crate::knowledge_time::parse_rfc3339(value).is_none())
                    || validation_event_id.is_some_and(|id| id.is_nil())
                {
                    return Err(Error::InvalidArguments);
                }
            }
            Self::SourceChanged {
                source_iri,
                accepted_digest,
                observed_digest,
            } => {
                if !bounded(source_iri, 4096)
                    || !digest(accepted_digest)
                    || !digest(observed_digest)
                    || accepted_digest == observed_digest
                {
                    return Err(Error::InvalidArguments);
                }
            }
            Self::DependencyChanged {
                dependency_unit_id,
                previous_event_id,
                observed_event_id,
            } => {
                if dependency_unit_id.is_nil()
                    || previous_event_id.is_some_and(|id| id.is_nil())
                    || observed_event_id.is_nil()
                    || previous_event_id.as_ref() == Some(observed_event_id)
                {
                    return Err(Error::InvalidArguments);
                }
            }
            Self::ApplicationFailed {
                consumer_ref,
                failure_digest,
            } => {
                if !bounded(consumer_ref, 4096) || !digest(failure_digest) {
                    return Err(Error::InvalidArguments);
                }
            }
            Self::OperatorRequested {
                subject_ref,
                observation_digest,
            } => {
                if !bounded(subject_ref, 4096) || !digest(observation_digest) {
                    return Err(Error::InvalidArguments);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeMaintenanceTaskState {
    Pending,
    Leased,
    NeedsReview,
    Linked,
    Resolved,
    Obsolete,
    Exhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeMaintenanceFailureCode {
    LeaseExpired,
    StorageUnavailable,
    TransportUnavailable,
    InvalidConfiguration,
    InternalInvariant,
    CapacityExceeded,
    NeedsContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeUnitReviewStatus {
    pub unit_id: Uuid,
    pub revision: i64,
    pub lifecycle: KnowledgeLifecycleState,
    pub access_scope: KnowledgeAccessScope,
    pub publication_event_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_event_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_due_at: Option<String>,
    pub due: bool,
    pub not_yet_valid: bool,
    pub expired: bool,
    pub needs_review: bool,
    #[serde(default)]
    pub maintenance_bases: Vec<KnowledgeMaintenanceReviewBasis>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeMaintenanceReviewBasis {
    pub task_id: Uuid,
    pub reason: KnowledgeMaintenanceSignalReason,
    pub basis_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeMaintenanceConsumer {
    pub consumer_ref: String,
    pub required: bool,
    pub relation_name: String,
    pub row_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeMaintenanceSignal {
    pub id: Uuid,
    pub unit_id: Uuid,
    pub unit_revision: i64,
    pub reason: KnowledgeMaintenanceSignalReason,
    pub basis: KnowledgeMaintenanceBasis,
    pub basis_digest: String,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeMaintenanceTerminalEvidence {
    pub change_id: Uuid,
    pub publisher_receipt_id: Uuid,
    pub operation_id: Uuid,
    pub event_id: Uuid,
    pub operation: KnowledgeLifecycleOperation,
    pub unit_revision: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_event_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeMaintenanceTask {
    pub id: Uuid,
    pub revision: i64,
    pub signal: KnowledgeMaintenanceSignal,
    pub state: KnowledgeMaintenanceTaskState,
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<KnowledgeMaintenanceFailureCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_retry_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_review: Option<KnowledgeUnitReviewStatus>,
    #[serde(default)]
    pub affected_consumers: Vec<KnowledgeMaintenanceConsumer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_evidence: Option<KnowledgeMaintenanceTerminalEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObserveKnowledgeMaintenanceSignal {
    pub request_id: Uuid,
    pub unit_id: Uuid,
    pub unit_revision: i64,
    pub basis: KnowledgeMaintenanceBasis,
}

impl ObserveKnowledgeMaintenanceSignal {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil() || self.unit_id.is_nil() || self.unit_revision < 1 {
            return Err(Error::InvalidArguments);
        }
        self.basis.validate()?;
        match self.basis {
            KnowledgeMaintenanceBasis::SourceChanged { .. }
            | KnowledgeMaintenanceBasis::ApplicationFailed { .. }
            | KnowledgeMaintenanceBasis::OperatorRequested { .. } => Ok(()),
            _ => Err(Error::InvalidArguments),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObserveKnowledgeMaintenanceOutcome {
    Created(KnowledgeMaintenanceTask),
    Existing(KnowledgeMaintenanceTask),
    Replay(KnowledgeMaintenanceTask),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeMaintenanceQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_id: Option<Uuid>,
    #[serde(default)]
    pub states: Vec<KnowledgeMaintenanceTaskState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Uuid>,
    pub limit: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fragment: Option<KnowledgeLifecycleFragmentQuery>,
}

impl KnowledgeMaintenanceQuery {
    pub fn validate(&self) -> Result<()> {
        if self.unit_id.is_some_and(|id| id.is_nil())
            || self.after.is_some_and(|id| id.is_nil())
            || self.limit == 0
            || self.limit > 100
            || self.states.len() > 7
        {
            return Err(Error::InvalidArguments);
        }
        if let Some(fragment) = &self.fragment {
            fragment.validate()?;
        }
        let mut states = self.states.clone();
        states.sort_by_key(|state| *state as u8);
        states.dedup();
        (states.len() == self.states.len())
            .then_some(())
            .ok_or(Error::InvalidArguments)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeMaintenanceContext {
    pub workspace_generation: i64,
    pub method: PipelineInstructionSnapshot,
    pub tasks: Vec<KnowledgeMaintenanceTask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_after: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeginKnowledgeMaintenanceChange {
    pub request_id: Uuid,
    pub task_id: Uuid,
    pub task_revision: i64,
    pub change: BeginKnowledgeChange,
}

impl BeginKnowledgeMaintenanceChange {
    pub fn validate(&self) -> Result<()> {
        self.change.validate()?;
        if self.request_id.is_nil()
            || self.task_id.is_nil()
            || self.task_revision < 1
            || self.change.request_id != self.request_id
            || self.change.owner != KnowledgeChangeOwner::Workspace
            || self.change.operation_hints.len() != 1
            || !matches!(
                self.change.operation_hints[0].operation,
                KnowledgeLifecycleOperation::Revalidate
                    | KnowledgeLifecycleOperation::Revise
                    | KnowledgeLifecycleOperation::Supersede
            )
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeginKnowledgeMaintenanceChangeOutcome {
    Created {
        task: KnowledgeMaintenanceTask,
        change: BeginKnowledgeChangeOutcome,
    },
    Replay {
        task: KnowledgeMaintenanceTask,
        change: BeginKnowledgeChangeOutcome,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeMaintenanceJobClaim {
    pub task_id: Uuid,
    pub task_revision: i64,
    pub lease_token: Uuid,
    pub workspace_id: Uuid,
    pub principal_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeMaintenanceClaimOutcome {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim: Option<KnowledgeMaintenanceJobClaim>,
    pub exhausted: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeMaintenancePrepareOutcome {
    NeedsReview,
    Obsolete,
    Exhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeMaintenanceFailureOutcome {
    RetryScheduled,
    NeedsReview,
    Exhausted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeMaintenanceProcessOutcome {
    pub due_created: u32,
    pub claimed: u32,
    pub needs_review: u32,
    pub obsolete: u32,
    pub exhausted: u32,
    pub retry_scheduled: u32,
    pub pending: u32,
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.as_bytes().contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observation_accepts_only_authenticated_external_signal_kinds() {
        let mut value = ObserveKnowledgeMaintenanceSignal {
            request_id: Uuid::new_v4(),
            unit_id: Uuid::new_v4(),
            unit_revision: 1,
            basis: KnowledgeMaintenanceBasis::OperatorRequested {
                subject_ref: "urn:subject".into(),
                observation_digest: "digest".into(),
            },
        };
        assert_eq!(value.validate(), Ok(()));
        value.basis = KnowledgeMaintenanceBasis::ReviewDue {
            review_due_at: Some("2026-09-15T00:00:00Z".into()),
            valid_until: None,
            validation_event_id: None,
        };
        assert_eq!(value.validate(), Err(Error::InvalidArguments));
    }
}
