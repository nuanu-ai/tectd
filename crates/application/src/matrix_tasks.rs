use crate::{TransactionMode, WorkspaceService};
use sha2::{Digest, Sha256};
use tect_domain::{
    EngineeringMatrixComposition, EngineeringMatrixInput, Error, RequestContext, Result,
    VerifiedEngineeringMatrixFacts, compose_engineering_matrix,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixTaskRevision {
    pub task_id: Uuid,
    pub revision: i64,
    pub request_id: Uuid,
    pub input: EngineeringMatrixInput,
    pub input_digest: String,
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
    let verified = VerifiedEngineeringMatrixFacts::bind_caller_verified_task_revision(
        revision.task_id.to_string(),
        revision.revision.to_string(),
        revision.input,
    )?;
    Ok(compose_engineering_matrix(&verified))
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
    request.input.validate()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::{MatrixFact, OperatingEnvelope, OperationalFacts};

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
}
