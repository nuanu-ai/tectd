use crate::{TransactionMode, WorkspaceService};
use sha2::{Digest, Sha256};
use tect_domain::{
    EngineeringChoiceSet, EngineeringMatrixComposition, EngineeringMatrixInput, Error,
    OwnerReportedEngineeringMatrixFacts, RequestContext, Result,
    compose_owner_reported_engineering_matrix,
};
use uuid::Uuid;

pub const MATRIX_INPUT_SCHEMA: &str = "tect.engineering-matrix-input/1";

/// The requested revision is exact: a new task starts at 1 and each edit
/// must name the immediate successor of the accepted revision.
#[derive(Debug, Clone)]
pub struct RecordMatrixTask {
    pub task_id: Uuid,
    pub revision: i64,
    pub expected_current_revision: i64,
    pub request_id: Uuid,
    pub input: EngineeringMatrixInput,
    pub choice_set: Option<EngineeringChoiceSet>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixTaskRevision {
    pub task_id: Uuid,
    pub revision: i64,
    pub request_id: Uuid,
    pub input: EngineeringMatrixInput,
    pub input_digest: String,
    pub choice_set: Option<EngineeringChoiceSet>,
    pub choice_set_digest: Option<String>,
    pub recorded_by_principal_id: Uuid,
    pub recorded_by_session_id: Uuid,
}

impl WorkspaceService {
    pub async fn record_matrix_task(
        &self,
        context: &RequestContext,
        request: &RecordMatrixTask,
    ) -> Result<MatrixTaskRevision> {
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        validate_request(request)?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        let canonical_input =
            serde_json::to_value(&request.input).map_err(|_| Error::InvalidArguments)?;
        let input_digest = canonical_matrix_input_digest(&canonical_input)?;
        let revision = tx
            .record_matrix_task(
                workspace.id,
                identity.principal_id,
                session.id,
                request,
                &canonical_input,
                &input_digest,
            )
            .await?;
        tx.commit().await?;
        Ok(revision)
    }

    pub async fn get_matrix_task(
        &self,
        context: &RequestContext,
        task_id: Uuid,
    ) -> Result<MatrixTaskRevision> {
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        if task_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let revision = tx
            .matrix_task(workspace.id, task_id)
            .await?
            .ok_or(Error::NotFound)?;
        tx.commit().await?;
        Ok(revision)
    }

    /// Compose cards from the accepted current task revision visible to this
    /// authenticated workspace member. This does not make a release decision.
    pub async fn compose_matrix_cards(
        &self,
        context: &RequestContext,
        task_id: Uuid,
        expected_task_revision: i64,
    ) -> Result<EngineeringMatrixComposition> {
        let revision = self.get_matrix_task(context, task_id).await?;
        compose_current_revision(revision, expected_task_revision)
    }
}

fn compose_current_revision(
    revision: MatrixTaskRevision,
    expected_task_revision: i64,
) -> Result<EngineeringMatrixComposition> {
    if expected_task_revision < 1 {
        return Err(Error::InvalidArguments);
    }
    if revision.revision != expected_task_revision {
        return Err(Error::StaleRevision);
    }
    let reported = OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
        revision.task_id.to_string(),
        revision.revision.to_string(),
        revision.input,
    )?;
    Ok(compose_owner_reported_engineering_matrix(&reported))
}

/// Hash the same canonical JSON representation that the store persists.
pub fn canonical_matrix_input_digest(input: &serde_json::Value) -> Result<String> {
    let encoded = serde_json::to_vec(input).map_err(|_| Error::InternalInvariant)?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

fn validate_request(request: &RecordMatrixTask) -> Result<()> {
    if request.task_id.is_nil()
        || request.request_id.is_nil()
        || request.revision < 1
        || request.expected_current_revision != request.revision - 1
    {
        return Err(Error::InvalidArguments);
    }
    request.input.validate()?;
    if let Some(choice_set) = &request.choice_set {
        if choice_set.task_id != request.task_id.to_string()
            || choice_set.task_revision != request.revision.to_string()
        {
            return Err(Error::InvalidArguments);
        }
        choice_set.validate(&request.input)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::{
        CommitmentEvidence, EngineeringCandidate, EngineeringIntent, EngineeringMode,
        FactProvenance, MATRIX_CHOICE_SET_SCHEMA, MatrixFact, MatrixSourceVerificationStatus,
        OperatingEnvelope, OperatingFact, OperationalFacts,
    };

    fn known<T>(value: T) -> MatrixFact<T> {
        MatrixFact::Known {
            value,
            provenance: FactProvenance("owner report".into()),
        }
    }

    fn stored_revision() -> MatrixTaskRevision {
        let task_id = Uuid::new_v4();
        let input = EngineeringMatrixInput {
            mode: MatrixFact::Absent,
            envelope: OperatingEnvelope {
                scale: MatrixFact::Absent,
                operational_facts: OperationalFacts::Absent,
            },
            criticality: MatrixFact::Absent,
            intent: MatrixFact::Absent,
            urgency: MatrixFact::Absent,
            promised_behavior: MatrixFact::Absent,
            promised_proof: MatrixFact::Absent,
            affected_guarantees: MatrixFact::Absent,
            actual_exposure: MatrixFact::Absent,
            demand_commitment: MatrixFact::Absent,
            latency_commitment: MatrixFact::Absent,
            urgent_repair: MatrixFact::Absent,
        };
        MatrixTaskRevision {
            task_id,
            revision: 2,
            request_id: Uuid::new_v4(),
            input,
            input_digest: "stored-digest".into(),
            choice_set: None,
            choice_set_digest: None,
            recorded_by_principal_id: Uuid::new_v4(),
            recorded_by_session_id: Uuid::new_v4(),
        }
    }

    #[test]
    fn composition_uses_exact_stored_revision_and_keeps_gaps_visible() {
        let stored = stored_revision();
        let output = compose_current_revision(stored.clone(), 2).unwrap();
        assert_eq!(output.task_id, stored.task_id.to_string());
        assert_eq!(output.task_revision, "2");
        assert_eq!(output.mandatory_cards[0].id, "EM02-SCOPE@0.1");
        assert!(!output.is_resolved());
    }

    #[test]
    fn complete_stored_demo_remains_pending_independent_verification() {
        let mut stored = stored_revision();
        stored.input = EngineeringMatrixInput {
            mode: known(EngineeringMode::Demo),
            envelope: OperatingEnvelope {
                scale: known("one synthetic request".into()),
                operational_facts: OperationalFacts::Reported {
                    entries: vec![OperatingFact {
                        name: "environment".into(),
                        fact: known("synthetic".into()),
                    }],
                },
            },
            criticality: known("no protected guarantee".into()),
            intent: known(EngineeringIntent::Other("demo".into())),
            urgency: known("ordinary".into()),
            promised_behavior: known("real demo".into()),
            promised_proof: known("demo check".into()),
            affected_guarantees: MatrixFact::KnownEmpty {
                provenance: FactProvenance("owner report".into()),
            },
            actual_exposure: known(false),
            demand_commitment: known(CommitmentEvidence::NoCommitment),
            latency_commitment: known(CommitmentEvidence::NoCommitment),
            urgent_repair: known(false),
        };
        let output = compose_current_revision(stored, 2).unwrap();
        assert!(output.unresolved_evidence.is_empty());
        assert_eq!(
            output.source_verification_status,
            MatrixSourceVerificationStatus::OwnerReportedPendingIndependentVerification
        );
        assert!(!output.is_resolved());
    }

    #[test]
    fn composition_rejects_stale_or_invalid_expected_revision() {
        let stored = stored_revision();
        assert_eq!(
            compose_current_revision(stored.clone(), 1),
            Err(Error::StaleRevision)
        );
        assert_eq!(
            compose_current_revision(stored, 0),
            Err(Error::InvalidArguments)
        );
    }

    #[test]
    fn canonical_digest_is_independent_of_json_object_key_order() {
        let left = serde_json::json!({"mode": {"known": {"value": "production", "provenance": "source"}}, "envelope": {"scale": "one"}});
        let right = serde_json::from_str::<serde_json::Value>(
            r#"{"envelope":{"scale":"one"},"mode":{"known":{"provenance":"source","value":"production"}}}"#,
        )
        .unwrap();
        assert_eq!(
            canonical_matrix_input_digest(&left).unwrap(),
            canonical_matrix_input_digest(&right).unwrap()
        );
    }

    #[test]
    fn choice_set_must_bind_to_exact_revision_and_input() {
        let stored = stored_revision();
        let mut request = RecordMatrixTask {
            task_id: stored.task_id,
            revision: 2,
            expected_current_revision: 1,
            request_id: stored.request_id,
            input: stored.input,
            choice_set: None,
        };
        assert_eq!(validate_request(&request), Ok(()));
        let choice = EngineeringChoiceSet {
            schema: MATRIX_CHOICE_SET_SCHEMA.into(),
            choice_set_id: "choice-1".into(),
            version: 1,
            task_id: request.task_id.to_string(),
            task_revision: "2".into(),
            decision_question: "Which approach?".into(),
            candidates: vec![EngineeringCandidate {
                candidate_id: "a".into(),
                title: "A".into(),
                approach: "Use A".into(),
                assumption_fact_ids: vec!["criticality".into()],
            }],
        };
        for count in 0..=1 {
            let mut choice = choice.clone();
            choice.candidates.truncate(count);
            request.choice_set = Some(choice);
            assert_eq!(validate_request(&request), Ok(()));
        }
        let mut choice = choice.clone();
        choice.task_revision = "1".into();
        request.choice_set = Some(choice.clone());
        assert_eq!(validate_request(&request), Err(Error::InvalidArguments));
        let mut choice = choice.clone();
        choice.task_revision = "2".into();
        choice.task_id = Uuid::new_v4().to_string();
        request.choice_set = Some(choice.clone());
        assert_eq!(validate_request(&request), Err(Error::InvalidArguments));
        let mut choice = choice;
        choice.task_id = request.task_id.to_string();
        choice.candidates[0].assumption_fact_ids = vec!["unknown.fact".into()];
        request.choice_set = Some(choice);
        assert_eq!(validate_request(&request), Err(Error::InvalidArguments));
    }
}
