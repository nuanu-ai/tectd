use crate::{
    CurrentMatrixAdvice, MatrixDispositionRecord, RecordMatrixDisposition, TransactionMode,
    WorkspaceService,
};
use tect_domain::{
    AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryOpportunityState, Error,
    MatrixDispositionBasis, MatrixDispositionDecision, RequestContext, Result,
};

impl WorkspaceService {
    pub async fn record_matrix_disposition(
        &self,
        context: &RequestContext,
        request: &RecordMatrixDisposition,
    ) -> Result<MatrixDispositionRecord> {
        request.validate()?;
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        if tx.session_principal(session.id).await? != identity.principal_id {
            return Err(Error::Forbidden);
        }
        if let Some(existing) = tx
            .matrix_disposition_by_request(workspace.id, request.request_id)
            .await?
        {
            if existing.request != *request
                || existing.recorded_by_principal_id != identity.principal_id
                || existing.recorded_by_session_id != session.id
                || existing.material_digest != request.material_digest()?
            {
                return Err(Error::InputConflict);
            }
            tx.commit().await?;
            return Ok(existing);
        }

        let revision = tx
            .lock_matrix_task(workspace.id, request.task_id)
            .await?
            .ok_or(Error::NotFound)?;
        if revision.revision != request.expected_task_revision
            || revision.input_digest != request.expected_input_digest
            || revision.choice_set_digest != request.expected_choice_set_digest
        {
            return Err(Error::StaleRevision);
        }
        if let MatrixDispositionDecision::Selected { selected_choice_id } = &request.decision {
            let choice_set = revision
                .choice_set
                .as_ref()
                .ok_or(Error::InvalidArguments)?;
            choice_set.validate(&revision.input)?;
            if !choice_set
                .candidates
                .iter()
                .any(|choice| choice.candidate_id == *selected_choice_id)
            {
                return Err(Error::InvalidArguments);
            }
        }

        let opportunity = tx
            .advisory_opportunity_for_dispatch(workspace.id, request.opportunity_id)
            .await?;
        if opportunity.workspace_id != workspace.id
            || opportunity.capability != AdvisoryCapability::EngineeringProfile
            || opportunity.decision_point
                != AdvisoryDecisionPoint::EngineeringProfileBeforeSelection
            || opportunity.target_kind != "matrix_task"
            || opportunity.target_id != Some(request.task_id)
            || opportunity.matrix_task_revision != Some(request.expected_task_revision)
            || opportunity.work_revision != Some(request.expected_task_revision)
            || opportunity.matrix_choice_set_digest != request.expected_choice_set_digest
            || opportunity.authorized_actor_id != identity.principal_id
            || opportunity.session_id != session.id
        {
            return Err(Error::StaleContext);
        }

        let current_advice: Option<CurrentMatrixAdvice> = match request.basis {
            MatrixDispositionBasis::NoCall
                if opportunity.state == AdvisoryOpportunityState::NoCall =>
            {
                None
            }
            MatrixDispositionBasis::Manual
                if matches!(
                    opportunity.state,
                    AdvisoryOpportunityState::NoCall
                        | AdvisoryOpportunityState::Failed
                        | AdvisoryOpportunityState::Invalidated
                ) =>
            {
                None
            }
            MatrixDispositionBasis::AfterAdvice
                if opportunity.state == AdvisoryOpportunityState::Advised =>
            {
                let stored = tx
                    .guarded_matrix_advice(workspace.id, opportunity.id)
                    .await?
                    .ok_or(Error::StaleContext)?;
                let config = tx.advisory_config(workspace.id).await?;
                let fresh =
                    super::matrix_tasks::compose_current_revision_with_validated_verification(
                        tx.matrix_verification_store(),
                        self.matrix_evidence_validator.as_ref(),
                        workspace.id,
                        revision.clone(),
                        request.expected_task_revision,
                        crate::matrix_verification::current_epoch_seconds()?,
                    )
                    .await
                    .ok()
                    .and_then(|(composition, verification)| {
                        verification.and_then(|verification| {
                            crate::MatrixProviderRequest::new_verified(
                                revision.clone(),
                                composition,
                                &verification,
                                stored.record.provider_profile_ref.clone(),
                                stored.record.model_configuration.clone(),
                            )
                            .ok()
                        })
                    });
                let advice = super::matrix_tasks::current_public_matrix_advice(
                    &opportunity,
                    &stored,
                    &config,
                    fresh.as_ref().map(|request| request.binding()),
                )
                .ok_or(Error::StaleContext)?;
                if request.advice_id != Some(advice.advice_id)
                    || request.advice_digest.as_deref() != Some(advice.advice_digest.as_str())
                {
                    return Err(Error::StaleContext);
                }
                Some(advice)
            }
            _ => return Err(Error::StaleContext),
        };
        let saved = tx
            .record_matrix_disposition(
                workspace.id,
                identity.principal_id,
                session.id,
                request,
                current_advice.as_ref(),
            )
            .await?;
        tx.commit().await?;
        Ok(saved)
    }
}
