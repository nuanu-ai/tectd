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

fn decode_disposition(row: PgRow) -> Result<MatrixDispositionRecord> {
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

#[async_trait]
impl MatrixDispositionStore for PgUnitOfWork {
    async fn matrix_disposition_by_request(
        &mut self,
        workspace_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<MatrixDispositionRecord>> {
        let tenant = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT d.disposition_id,d.request_id,d.actor_id,d.session_id,d.opportunity_id, \
                    d.task_id,d.matrix_task_revision,d.matrix_choice_set_digest,d.basis, \
                    d.advice_id,d.outcome,d.selected_choice_id,d.blocked_reason, \
                    r.input_digest,a.advice_digest \
             FROM advisory_matrix_disposition d \
             JOIN matrix_task_revisions r ON (r.tenant_id,r.workspace_id,r.task_id,r.revision)= \
               (d.tenant_id,d.workspace_id,d.task_id,d.matrix_task_revision) \
             LEFT JOIN advisory_matrix_advice a ON (a.tenant_id,a.workspace_id,a.advice_id)= \
               (d.tenant_id,d.workspace_id,d.advice_id) \
             WHERE d.tenant_id=$1 AND d.workspace_id=$2 AND d.request_id=$3",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(request_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        row.map(decode_disposition).transpose()
    }

    async fn record_matrix_disposition(
        &mut self,
        workspace_id: Uuid,
        actor_id: Uuid,
        session_id: Uuid,
        request: &RecordMatrixDisposition,
        current_advice: Option<&CurrentMatrixAdvice>,
        current_verification: Option<&RevalidatedMatrixVerification>,
    ) -> Result<MatrixDispositionRecord> {
        request.validate()?;
        if workspace_id.is_nil()
            || actor_id.is_nil()
            || session_id.is_nil()
            || self.principal_id()? != actor_id
        {
            return Err(Error::Forbidden);
        }
        let tenant = self.tenant_id()?;
        // The runtime role cannot lock private host/principal rows. These
        // existing security-definer reads provide preflight; the INSERT trigger
        // repeats the full check with row locks until transaction commit.
        let active: bool = sqlx::query_scalar(
            "SELECT COALESCE(public.tect_dk_session_principal($1)=$2,false) \
                    AND public.tect_dk_is_owner($2) \
                    AND EXISTS(SELECT 1 FROM memberships m \
                      WHERE m.tenant_id=$3 AND m.workspace_id=$4 AND m.principal_id=$2)",
        )
        .bind(session_id)
        .bind(actor_id)
        .bind(tenant)
        .bind(workspace_id)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        if !active {
            return Err(Error::Forbidden);
        }
        if let Some(prior) = self
            .matrix_disposition_by_request(workspace_id, request.request_id)
            .await?
        {
            return if same_request(&prior, actor_id, session_id, request) {
                Ok(prior)
            } else {
                Err(Error::InputConflict)
            };
        }

        let opportunity = sqlx::query(
            "SELECT work_item_kind,work_item_id,source_revision,matrix_task_revision, \
                    matrix_choice_set_digest,matrix_verification_digest,session_id,authorized_actor_id, \
                    capability,decision_point,config_revision,material_digest,state,primary_reason \
             FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE",
        )
        .bind(tenant).bind(workspace_id).bind(request.opportunity_id)
        .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?
        .ok_or(Error::NotFound)?;
        let revision = request.expected_task_revision;
        let state: String = opportunity.try_get("state").map_err(storage_error)?;
        if opportunity
            .try_get::<String, _>("work_item_kind")
            .map_err(storage_error)?
            != "matrix_task"
            || opportunity
                .try_get::<Option<Uuid>, _>("work_item_id")
                .map_err(storage_error)?
                != Some(request.task_id)
            || opportunity
                .try_get::<Option<String>, _>("source_revision")
                .map_err(storage_error)?
                .as_deref()
                != Some(revision.to_string().as_str())
            || opportunity
                .try_get::<Option<i64>, _>("matrix_task_revision")
                .map_err(storage_error)?
                != Some(revision)
            || opportunity
                .try_get::<Option<String>, _>("matrix_choice_set_digest")
                .map_err(storage_error)?
                != request.expected_choice_set_digest
            || opportunity
                .try_get::<Uuid, _>("session_id")
                .map_err(storage_error)?
                != session_id
            || opportunity
                .try_get::<Uuid, _>("authorized_actor_id")
                .map_err(storage_error)?
                != actor_id
            || opportunity
                .try_get::<String, _>("capability")
                .map_err(storage_error)?
                != "engineering_profile"
            || opportunity
                .try_get::<String, _>("decision_point")
                .map_err(storage_error)?
                != "engineering.profile.before_selection"
        {
            return Err(Error::StaleContext);
        }
        match request.basis {
            MatrixDispositionBasis::NoCall if state == "no_call" => {}
            MatrixDispositionBasis::Manual
                if matches!(state.as_str(), "no_call" | "failed" | "invalidated") => {}
            MatrixDispositionBasis::AfterAdvice if state == "advised" => {}
            _ => return Err(Error::StaleContext),
        }

        let current = self
            .lock_matrix_task(workspace_id, request.task_id)
            .await?
            .ok_or(Error::NotFound)?;
        if current.revision != revision
            || current.input_digest != request.expected_input_digest
            || current.choice_set_digest != request.expected_choice_set_digest
        {
            return Err(Error::StaleRevision);
        }
        if let MatrixDispositionDecision::Selected { selected_choice_id } = &request.decision {
            let set = current.choice_set.as_ref().ok_or(Error::InvalidArguments)?;
            set.validate(&current.input)?;
            if !set
                .candidates
                .iter()
                .any(|candidate| candidate.candidate_id == *selected_choice_id)
            {
                return Err(Error::InvalidArguments);
            }
            let token = current_verification.ok_or(Error::StaleContext)?;
            let digest = token.record_digest();
            let saved = self
                .matrix_verification_for_revision(
                    workspace_id,
                    request.task_id,
                    revision,
                    &request.expected_input_digest,
                )
                .await?
                .ok_or(Error::StaleContext)?;
            if saved.digest != digest
                || saved.owner_principal != current.recorded_by_principal_id.to_string()
                || saved.verifier_principal == saved.owner_principal
            {
                return Err(Error::StaleContext);
            }
            let now: i64 = sqlx::query_scalar(
                "SELECT FLOOR(EXTRACT(EPOCH FROM pg_catalog.clock_timestamp()))::bigint",
            )
            .fetch_one(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
            let validated = evaluate_matrix_verification(
                &request.task_id.to_string(),
                &revision.to_string(),
                &current.input,
                &saved,
                now,
            )
            .map_err(|_| Error::StaleContext)?;
            let reported = OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
                request.task_id.to_string(),
                revision.to_string(),
                current.input.clone(),
            )
            .map_err(|_| Error::StaleContext)?;
            let composition = compose_independently_verified_owner_matrix(&reported, &validated)
                .map_err(|_| Error::StaleContext)?;
            let evaluation =
                matrix_verified_disposition_digest(&current.input, &composition, set, &validated)
                    .map_err(|_| Error::StaleContext)?;
            let reason: String = opportunity
                .try_get("primary_reason")
                .map_err(storage_error)?;
            let captured_verification: Option<String> = opportunity
                .try_get("matrix_verification_digest")
                .map_err(storage_error)?;
            let captured_material: String = opportunity
                .try_get("material_digest")
                .map_err(storage_error)?;
            if !selected_receipt_matches(
                &reason,
                captured_verification.as_deref(),
                digest,
                &captured_material,
                &evaluation,
            ) {
                return Err(Error::StaleContext);
            }
        } else if current_verification.is_some() {
            return Err(Error::StaleContext);
        }

        if request.basis == MatrixDispositionBasis::AfterAdvice {
            let token = current_advice.ok_or(Error::StaleContext)?;
            let advice = sqlx::query(
                "SELECT a.advice_id,a.dispatch_id,a.kind,a.ranked_choice_ids,a.reason, \
                        a.advice_digest,a.provider_profile_ref, \
                        a.model_configuration,a.response_payload_sha256,a.matrix_choice_set_digest, \
                        d.opportunity_id,d.provider,d.model,d.configuration_snapshot,d.configuration_digest, \
                        d.material_digest,d.state,d.send_certainty,d.outcome,d.response_payload, \
                        c.revision AS current_config_revision,c.mode,c.provider_profile_ref AS current_profile, \
                        c.model_configuration AS current_model \
                 FROM advisory_matrix_advice a \
                 JOIN advisory_dispatch d ON (d.tenant_id,d.workspace_id,d.id)= \
                   (a.tenant_id,a.workspace_id,a.dispatch_id) \
                 JOIN advisory_workspace_config c ON (c.tenant_id,c.workspace_id)= \
                   (a.tenant_id,a.workspace_id) \
                 WHERE a.tenant_id=$1 AND a.workspace_id=$2 AND a.opportunity_id=$3 \
                 FOR SHARE OF d,c",
            )
            .bind(tenant).bind(workspace_id).bind(request.opportunity_id)
            .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?
            .ok_or(Error::StaleContext)?;
            let response: Option<Vec<u8>> =
                advice.try_get("response_payload").map_err(storage_error)?;
            let snapshot: serde_json::Value = advice
                .try_get("configuration_snapshot")
                .map_err(storage_error)?;
            let config_sha = format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&snapshot).map_err(storage_error)?)
            );
            let stored_outcome = match (
                advice
                    .try_get::<String, _>("kind")
                    .map_err(storage_error)?
                    .as_str(),
                advice
                    .try_get::<Option<serde_json::Value>, _>("ranked_choice_ids")
                    .map_err(storage_error)?,
                advice
                    .try_get::<Option<String>, _>("reason")
                    .map_err(storage_error)?,
            ) {
                ("ranked", Some(ranks), None) => GuardedMatrixAdviceOutcome::Ranked {
                    ranked_choice_ids: serde_json::from_value(ranks)
                        .map_err(|_| Error::StaleContext)?,
                },
                ("abstained", None, reason) => GuardedMatrixAdviceOutcome::Abstained { reason },
                _ => return Err(Error::StaleContext),
            };
            if request.advice_id != Some(token.advice_id)
                || request.advice_digest.as_deref() != Some(token.advice_digest.as_str())
                || token.task_revision != revision
                || token.input_digest != request.expected_input_digest
                || Some(token.choice_set_digest.as_str())
                    != request.expected_choice_set_digest.as_deref()
                || token.choice_set_id
                    != current
                        .choice_set
                        .as_ref()
                        .ok_or(Error::StaleContext)?
                        .choice_set_id
                || token.choice_set_version
                    != current
                        .choice_set
                        .as_ref()
                        .ok_or(Error::StaleContext)?
                        .version
                || opportunity
                    .try_get::<i64, _>("config_revision")
                    .map_err(storage_error)?
                    != advice
                        .try_get::<i64, _>("current_config_revision")
                        .map_err(storage_error)?
                || advice.try_get::<String, _>("mode").map_err(storage_error)? != "optional"
                || advice
                    .try_get::<Uuid, _>("advice_id")
                    .map_err(storage_error)?
                    != token.advice_id
                || advice
                    .try_get::<Uuid, _>("dispatch_id")
                    .map_err(storage_error)?
                    != token.dispatch_id
                || advice
                    .try_get::<String, _>("advice_digest")
                    .map_err(storage_error)?
                    != token.advice_digest
                || advice
                    .try_get::<String, _>("matrix_choice_set_digest")
                    .map_err(storage_error)?
                    != token.choice_set_digest
                || stored_outcome != token.outcome
                || advice
                    .try_get::<Uuid, _>("opportunity_id")
                    .map_err(storage_error)?
                    != request.opportunity_id
                || advice
                    .try_get::<String, _>("provider")
                    .map_err(storage_error)?
                    != token.provider_profile_ref.id
                || advice
                    .try_get::<String, _>("model")
                    .map_err(storage_error)?
                    != token.model_configuration.model
                || advice
                    .try_get::<String, _>("provider_profile_ref")
                    .map_err(storage_error)?
                    != token.provider_profile_ref.id
                || advice
                    .try_get::<Option<String>, _>("current_profile")
                    .map_err(storage_error)?
                    .as_deref()
                    != Some(token.provider_profile_ref.id.as_str())
                || advice
                    .try_get::<serde_json::Value, _>("model_configuration")
                    .map_err(storage_error)?
                    != serde_json::json!(token.model_configuration)
                || advice
                    .try_get::<Option<serde_json::Value>, _>("current_model")
                    .map_err(storage_error)?
                    != Some(serde_json::json!(token.model_configuration))
                || advice
                    .try_get::<String, _>("configuration_digest")
                    .map_err(storage_error)?
                    != config_sha
                || advice
                    .try_get::<String, _>("material_digest")
                    .map_err(storage_error)?
                    != token.evaluation_digest
                || opportunity
                    .try_get::<String, _>("material_digest")
                    .map_err(storage_error)?
                    != token.evaluation_digest
                || opportunity
                    .try_get::<Option<String>, _>("matrix_verification_digest")
                    .map_err(storage_error)?
                    .as_deref()
                    != Some(token.verification_digest.as_str())
                || advice
                    .try_get::<String, _>("state")
                    .map_err(storage_error)?
                    != "sealed"
                || advice
                    .try_get::<String, _>("send_certainty")
                    .map_err(storage_error)?
                    != "sent"
                || advice
                    .try_get::<Option<String>, _>("outcome")
                    .map_err(storage_error)?
                    .as_deref()
                    != Some("provider_response")
                || advice
                    .try_get::<String, _>("response_payload_sha256")
                    .map_err(storage_error)?
                    != token.response_payload_sha256
                || response
                    .as_ref()
                    .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
                    != Some(token.response_payload_sha256.clone())
                || snapshot.get("provider_profile_ref")
                    != Some(&serde_json::json!(token.provider_profile_ref))
                || snapshot.get("model_configuration")
                    != Some(&serde_json::json!(token.model_configuration))
            {
                return Err(Error::StaleContext);
            }
            let fresh: Option<bool> = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM matrix_verifications v \
                 WHERE v.tenant_id=$1 AND v.workspace_id=$2 AND v.task_id=$3 \
                   AND v.task_revision=$4 AND v.input_digest=$5 AND v.record_digest=$6 \
                   AND EXISTS(SELECT 1 FROM matrix_verification_bindings b \
                     WHERE b.tenant_id=v.tenant_id AND b.workspace_id=v.workspace_id \
                       AND b.verification_id=v.id) \
                   AND NOT EXISTS(SELECT 1 FROM matrix_verification_bindings b \
                     WHERE b.tenant_id=v.tenant_id AND b.workspace_id=v.workspace_id \
                       AND b.verification_id=v.id \
                       AND b.expires_at<=FLOOR(EXTRACT(EPOCH FROM pg_catalog.clock_timestamp()))::bigint))",
            ).bind(tenant).bind(workspace_id).bind(request.task_id).bind(revision)
             .bind(&request.expected_input_digest).bind(&token.verification_digest)
             .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
            if fresh != Some(true) {
                return Err(Error::StaleContext);
            }
        } else if current_advice.is_some()
            || request.advice_id.is_some()
            || request.advice_digest.is_some()
        {
            return Err(Error::StaleContext);
        }

        let (outcome, selected, blocked) = match &request.decision {
            MatrixDispositionDecision::Selected { selected_choice_id } => {
                ("selected", Some(selected_choice_id.as_str()), None)
            }
            MatrixDispositionDecision::Blocked { blocked_reason } => {
                ("blocked", None, Some(blocked_reason.as_str()))
            }
        };
        let inserted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO advisory_matrix_disposition \
               (tenant_id,workspace_id,opportunity_id,task_id,matrix_task_revision, \
                matrix_choice_set_digest,request_id,actor_id,session_id,basis,advice_id, \
                outcome,selected_choice_id,blocked_reason) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) \
             ON CONFLICT DO NOTHING RETURNING disposition_id",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(request.opportunity_id)
        .bind(request.task_id)
        .bind(revision)
        .bind(&request.expected_choice_set_digest)
        .bind(request.request_id)
        .bind(actor_id)
        .bind(session_id)
        .bind(request.basis.as_str())
        .bind(request.advice_id)
        .bind(outcome)
        .bind(selected)
        .bind(blocked)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(disposition_write_error)?;
        match inserted {
            Some(disposition_id) => Ok(MatrixDispositionRecord {
                disposition_id,
                request: request.clone(),
                recorded_by_principal_id: actor_id,
                recorded_by_session_id: session_id,
                material_digest: request.material_digest()?,
            }),
            None => {
                self.disposition_retry_or_error(
                    workspace_id,
                    actor_id,
                    session_id,
                    request,
                    Error::InputConflict,
                )
                .await
            }
        }
    }
}

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
