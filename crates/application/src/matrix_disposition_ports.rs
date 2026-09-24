use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tect_domain::{Error, MatrixDispositionBasis, MatrixDispositionDecision, Result};
use uuid::Uuid;

use crate::{CurrentMatrixAdvice, RevalidatedMatrixVerification};

/// Agent-authored decision over an exact saved Matrix task and opportunity.
/// Actor identity is deliberately absent; it comes from the authenticated session.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordMatrixDisposition {
    pub request_id: Uuid,
    pub task_id: Uuid,
    pub expected_task_revision: i64,
    pub expected_input_digest: String,
    pub expected_choice_set_digest: Option<String>,
    pub opportunity_id: Uuid,
    pub basis: MatrixDispositionBasis,
    pub advice_id: Option<Uuid>,
    pub advice_digest: Option<String>,
    pub decision: MatrixDispositionDecision,
}

impl RecordMatrixDisposition {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.task_id.is_nil()
            || self.opportunity_id.is_nil()
            || self.expected_task_revision < 1
            || !valid_digest(&self.expected_input_digest)
            || self
                .expected_choice_set_digest
                .as_deref()
                .is_some_and(|d| !valid_digest(d))
            || self
                .advice_digest
                .as_deref()
                .is_some_and(|d| !valid_digest(d))
        {
            return Err(Error::InvalidArguments);
        }
        match self.basis {
            MatrixDispositionBasis::AfterAdvice
                if self.advice_id.is_some_and(|id| !id.is_nil())
                    && self.advice_digest.is_some() => {}
            MatrixDispositionBasis::NoCall | MatrixDispositionBasis::Manual
                if self.advice_id.is_none() && self.advice_digest.is_none() => {}
            _ => return Err(Error::InvalidArguments),
        }
        self.decision.validate()?;
        if matches!(&self.decision, MatrixDispositionDecision::Selected { .. })
            && self.expected_choice_set_digest.is_none()
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }

    pub fn material_digest(&self) -> Result<String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| Error::InternalInvariant)?;
        let mut hash = Sha256::new();
        hash.update(b"tect.matrix-disposition/1\0");
        hash.update(bytes);
        Ok(format!("{:x}", hash.finalize()))
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixDispositionRecord {
    pub disposition_id: Uuid,
    pub request: RecordMatrixDisposition,
    pub recorded_by_principal_id: Uuid,
    pub recorded_by_session_id: Uuid,
    pub material_digest: String,
}

/// The adapter must compare an existing request ID with every material field,
/// actor and session, returning the original row only for exact replay.
/// For a new row, lock the task head, opportunity, guarded advice and config in
/// one transaction; recheck task/input/choice digests, opportunity lifecycle,
/// actor/session binding, selected choice membership and the current-advice
/// token against saved dispatch/advice/config/evidence. Insert immutably.
/// The app's preliminary checks do not replace these atomic adapter guards.
#[async_trait]
pub trait MatrixDispositionStore: Send {
    async fn matrix_disposition_by_request(
        &mut self,
        workspace_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<MatrixDispositionRecord>>;

    async fn record_matrix_disposition(
        &mut self,
        workspace_id: Uuid,
        actor_id: Uuid,
        session_id: Uuid,
        request: &RecordMatrixDisposition,
        current_advice: Option<&CurrentMatrixAdvice>,
        current_verification: Option<&RevalidatedMatrixVerification>,
    ) -> Result<MatrixDispositionRecord>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> RecordMatrixDisposition {
        RecordMatrixDisposition {
            request_id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            expected_task_revision: 2,
            expected_input_digest: "a".repeat(64),
            expected_choice_set_digest: Some("b".repeat(64)),
            opportunity_id: Uuid::new_v4(),
            basis: MatrixDispositionBasis::NoCall,
            advice_id: None,
            advice_digest: None,
            decision: MatrixDispositionDecision::Blocked {
                blocked_reason: "Await independent evidence".into(),
            },
        }
    }

    /// Models the persistence port's request-id guard without a database.
    #[derive(Default)]
    struct FakeDispositionStore(Option<MatrixDispositionRecord>);

    #[async_trait]
    impl MatrixDispositionStore for FakeDispositionStore {
        async fn matrix_disposition_by_request(
            &mut self,
            _workspace_id: Uuid,
            request_id: Uuid,
        ) -> Result<Option<MatrixDispositionRecord>> {
            Ok(self
                .0
                .as_ref()
                .filter(|saved| saved.request.request_id == request_id)
                .cloned())
        }

        async fn record_matrix_disposition(
            &mut self,
            _workspace_id: Uuid,
            actor_id: Uuid,
            session_id: Uuid,
            request: &RecordMatrixDisposition,
            current_advice: Option<&CurrentMatrixAdvice>,
            current_verification: Option<&RevalidatedMatrixVerification>,
        ) -> Result<MatrixDispositionRecord> {
            request.validate()?;
            if request.basis == MatrixDispositionBasis::AfterAdvice && current_advice.is_none() {
                return Err(Error::StaleContext);
            }
            if matches!(
                &request.decision,
                MatrixDispositionDecision::Selected { .. }
            ) && current_verification.is_none()
            {
                return Err(Error::StaleContext);
            }
            if let Some(saved) = &self.0 {
                if saved.request != *request
                    || saved.recorded_by_principal_id != actor_id
                    || saved.recorded_by_session_id != session_id
                    || saved.material_digest != request.material_digest()?
                {
                    return Err(Error::InputConflict);
                }
                return Ok(saved.clone());
            }
            let saved = MatrixDispositionRecord {
                disposition_id: Uuid::new_v4(),
                request: request.clone(),
                recorded_by_principal_id: actor_id,
                recorded_by_session_id: session_id,
                material_digest: request.material_digest()?,
            };
            self.0 = Some(saved.clone());
            Ok(saved)
        }
    }

    #[tokio::test]
    async fn fake_port_replays_exact_material_and_rejects_changed_choice_or_actor() {
        let mut store = FakeDispositionStore::default();
        let workspace = Uuid::new_v4();
        let actor = Uuid::new_v4();
        let session = Uuid::new_v4();
        let request = request();
        let first = store
            .record_matrix_disposition(workspace, actor, session, &request, None, None)
            .await
            .unwrap();
        assert_eq!(
            store
                .matrix_disposition_by_request(workspace, request.request_id)
                .await
                .unwrap(),
            Some(first.clone())
        );
        assert_eq!(
            store
                .record_matrix_disposition(workspace, actor, session, &request, None, None)
                .await
                .unwrap(),
            first
        );
        let mut changed = request.clone();
        changed.decision = MatrixDispositionDecision::Blocked {
            blocked_reason: "Different blocker".into(),
        };
        assert_eq!(
            store
                .record_matrix_disposition(workspace, actor, session, &changed, None, None)
                .await,
            Err(Error::InputConflict)
        );
        assert_eq!(
            store
                .record_matrix_disposition(workspace, Uuid::new_v4(), session, &request, None, None)
                .await,
            Err(Error::InputConflict)
        );
    }

    #[tokio::test]
    async fn advice_basis_requires_advice_and_blocked_requires_reason() {
        let mut request = request();
        request.basis = MatrixDispositionBasis::AfterAdvice;
        request.advice_id = Some(Uuid::new_v4());
        request.advice_digest = Some("c".repeat(64));
        assert_eq!(
            FakeDispositionStore::default()
                .record_matrix_disposition(
                    Uuid::new_v4(),
                    Uuid::new_v4(),
                    Uuid::new_v4(),
                    &request,
                    None,
                    None
                )
                .await,
            Err(Error::StaleContext)
        );
        request.basis = MatrixDispositionBasis::Manual;
        assert_eq!(request.validate(), Err(Error::InvalidArguments));
        request.advice_id = None;
        request.advice_digest = None;
        request.decision = MatrixDispositionDecision::Blocked {
            blocked_reason: " ".into(),
        };
        assert_eq!(request.validate(), Err(Error::InvalidArguments));

        request.decision = MatrixDispositionDecision::Selected {
            selected_choice_id: "choice-b".into(),
        };
        assert_eq!(
            FakeDispositionStore::default()
                .record_matrix_disposition(
                    Uuid::new_v4(),
                    Uuid::new_v4(),
                    Uuid::new_v4(),
                    &request,
                    None,
                    None,
                )
                .await,
            Err(Error::StaleContext)
        );
    }
}
