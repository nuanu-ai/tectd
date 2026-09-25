use async_trait::async_trait;
use sha2::{Digest, Sha256};
use sqlx::{Row, postgres::PgRow};
use tect_application::{
    CurrentMatrixAdvice, GuardedMatrixAdviceOutcome, MatrixDispositionRecord,
    MatrixDispositionStore, MatrixTaskStore, MatrixVerificationStore, RecordMatrixDisposition,
    RevalidatedMatrixVerification,
};
use tect_domain::{
    Error, MatrixDispositionBasis, MatrixDispositionDecision, OwnerReportedEngineeringMatrixFacts,
    Result, compose_independently_verified_owner_matrix, evaluate_matrix_verification,
    matrix_verified_disposition_digest,
};
use uuid::Uuid;

use crate::{storage_error, store::PgUnitOfWork};

fn disposition_write_error(error: sqlx::Error) -> Error {
    if error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| code.as_ref() == "42501")
    {
        Error::Forbidden
    } else {
        storage_error(error)
    }
}

pub(crate) fn decode_disposition(row: PgRow) -> Result<MatrixDispositionRecord> {
    let basis = match row
        .try_get::<String, _>("basis")
        .map_err(storage_error)?
        .as_str()
    {
        "after_advice" => MatrixDispositionBasis::AfterAdvice,
        "no_call" => MatrixDispositionBasis::NoCall,
        "manual" => MatrixDispositionBasis::Manual,
        _ => return Err(Error::InternalInvariant),
    };
    let selected: Option<String> = row.try_get("selected_choice_id").map_err(storage_error)?;
    let blocked: Option<String> = row.try_get("blocked_reason").map_err(storage_error)?;
    let decision = match (
        row.try_get::<String, _>("outcome")
            .map_err(storage_error)?
            .as_str(),
        selected,
        blocked,
    ) {
        ("selected", Some(selected_choice_id), None) => {
            MatrixDispositionDecision::Selected { selected_choice_id }
        }
        ("blocked", None, Some(blocked_reason)) => {
            MatrixDispositionDecision::Blocked { blocked_reason }
        }
        _ => return Err(Error::InternalInvariant),
    };
    let request = RecordMatrixDisposition {
        request_id: row.try_get("request_id").map_err(storage_error)?,
        task_id: row.try_get("task_id").map_err(storage_error)?,
        expected_task_revision: row.try_get("matrix_task_revision").map_err(storage_error)?,
        expected_input_digest: row.try_get("input_digest").map_err(storage_error)?,
        expected_choice_set_digest: row
            .try_get("matrix_choice_set_digest")
            .map_err(storage_error)?,
        opportunity_id: row.try_get("opportunity_id").map_err(storage_error)?,
        basis,
        advice_id: row.try_get("advice_id").map_err(storage_error)?,
        advice_digest: row.try_get("advice_digest").map_err(storage_error)?,
        decision,
    };
    let material_digest = request
        .material_digest()
        .map_err(|_| Error::InternalInvariant)?;
    Ok(MatrixDispositionRecord {
        disposition_id: row.try_get("disposition_id").map_err(storage_error)?,
        request,
        recorded_by_principal_id: row.try_get("actor_id").map_err(storage_error)?,
        recorded_by_session_id: row.try_get("session_id").map_err(storage_error)?,
        material_digest,
    })
}

fn same_request(
    prior: &MatrixDispositionRecord,
    actor_id: Uuid,
    session_id: Uuid,
    request: &RecordMatrixDisposition,
) -> bool {
    prior.request == *request
        && prior.recorded_by_principal_id == actor_id
        && prior.recorded_by_session_id == session_id
}

fn selected_receipt_matches(
    reason: &str,
    captured_verification_digest: Option<&str>,
    current_verification_digest: &str,
    captured_material_digest: &str,
    recomposed_material_digest: &str,
) -> bool {
    !matches!(
        reason,
        "matrix_evidence_unresolved" | "matrix_source_unverified"
    ) && captured_verification_digest == Some(current_verification_digest)
        && captured_material_digest == recomposed_material_digest
}

impl PgUnitOfWork {
    async fn disposition_retry_or_error(
        &mut self,
        workspace_id: Uuid,
        actor_id: Uuid,
        session_id: Uuid,
        request: &RecordMatrixDisposition,
        fallback: Error,
    ) -> Result<MatrixDispositionRecord> {
        match self
            .matrix_disposition_by_request(workspace_id, request.request_id)
            .await?
        {
            Some(prior) if same_request(&prior, actor_id, session_id, request) => Ok(prior),
            Some(_) => Err(Error::InputConflict),
            None => Err(fallback),
        }
    }
}

mod implementation;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_requires_exact_request_and_authenticated_identity() {
        let actor = Uuid::new_v4();
        let session = Uuid::new_v4();
        let request = RecordMatrixDisposition {
            request_id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            expected_task_revision: 2,
            expected_input_digest: "a".repeat(64),
            expected_choice_set_digest: Some("b".repeat(64)),
            opportunity_id: Uuid::new_v4(),
            basis: MatrixDispositionBasis::NoCall,
            advice_id: None,
            advice_digest: None,
            decision: MatrixDispositionDecision::Selected {
                selected_choice_id: "owner-choice".into(),
            },
        };
        let prior = MatrixDispositionRecord {
            disposition_id: Uuid::new_v4(),
            material_digest: request.material_digest().unwrap(),
            request: request.clone(),
            recorded_by_principal_id: actor,
            recorded_by_session_id: session,
        };
        assert!(same_request(&prior, actor, session, &request));
        assert!(!same_request(&prior, Uuid::new_v4(), session, &request));
        assert!(!same_request(&prior, actor, Uuid::new_v4(), &request));
        let mut changed = request;
        changed.expected_input_digest = "c".repeat(64);
        assert!(!same_request(&prior, actor, session, &changed));
        changed = prior.request.clone();
        changed.decision = MatrixDispositionDecision::Blocked {
            blocked_reason: "new decision".into(),
        };
        assert!(!same_request(&prior, actor, session, &changed));
    }

    #[test]
    fn selected_skip_requires_captured_current_verified_material() {
        let verification = "a".repeat(64);
        let singleton_material = "b".repeat(64);
        assert!(selected_receipt_matches(
            "request_skip",
            Some(&verification),
            &verification,
            &singleton_material,
            &singleton_material,
        ));
        assert!(!selected_receipt_matches(
            "request_skip",
            None,
            &verification,
            &singleton_material,
            &singleton_material,
        ));
        assert!(!selected_receipt_matches(
            "matrix_evidence_unresolved",
            Some(&verification),
            &verification,
            &singleton_material,
            &singleton_material,
        ));
        assert!(!selected_receipt_matches(
            "request_skip",
            Some(&verification),
            &verification,
            &singleton_material,
            &"c".repeat(64),
        ));
    }
}
