use crate::{
    CurrentMatrixAdvice, MatrixDispositionRecord, MatrixDispositionVerification,
    RecordMatrixDisposition, TransactionMode, WorkspaceService,
};
use tect_domain::{
    AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryOpportunity, AdvisoryOpportunityState,
    AdvisoryReason, ENGINEERING_MATRIX_CATALOGUE_VERSION, EngineeringMatrixComposition, Error,
    MatrixDispositionBasis, MatrixDispositionDecision, MatrixSourceVerificationStatus,
    PrincipalRole, RequestContext, Result,
};
use uuid::Uuid;

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

        // Declaration locks precede the task lock, matching task recording and
        // advisory capture. A changed task head is rejected after both reads.
        let source = tx.matrix_task_source(workspace.id, request.task_id).await?;
        let bound_context = if let Some(binding) = source
            .as_ref()
            .and_then(|source| source.requirements_binding.as_ref())
        {
            Some(
                crate::matrix_verification::lock_and_load_bound_matrix_context(
                    tx.matrix_requirements_context_store()
                        .ok_or(Error::StaleContext)?,
                    workspace.id,
                    identity.principal_id,
                    binding,
                )
                .await
                .map_err(|_| Error::StaleContext)?,
            )
        } else {
            None
        };
        let revision = tx
            .lock_matrix_task(workspace.id, request.task_id)
            .await?
            .ok_or(Error::NotFound)?;
        if source
            .as_ref()
            .is_none_or(|source| source.revision != revision)
        {
            return Err(Error::StaleRevision);
        }
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

        let selected = matches!(
            &request.decision,
            MatrixDispositionDecision::Selected { .. }
        );
        let validated_snapshot = if requires_current_verification(request) {
            let now = crate::matrix_verification::current_epoch_seconds()?;
            if let (Some(binding), Some(context)) = (
                source
                    .as_ref()
                    .and_then(|source| source.requirements_binding.as_ref()),
                bound_context.as_ref(),
            ) {
                let (composition, record) =
                    super::matrix_tasks::compose_bound_revision_with_verification(
                        tx.context_matrix_verification_store(),
                        self.matrix_evidence_validator.as_ref(),
                        workspace.id,
                        &revision,
                        binding.snapshot_id,
                        context,
                        now,
                    )
                    .await
                    .map_err(|_| Error::StaleContext)?
                    .ok_or(Error::StaleContext)?;
                Some(MatrixDispositionVerification::ContextV2 {
                    binding: binding.clone(),
                    composition: Box::new(composition),
                    record: Box::new(record),
                })
            } else {
                let (composition, verification) =
                    super::matrix_tasks::compose_current_revision_with_validated_verification(
                        tx.matrix_verification_store(),
                        self.matrix_evidence_validator.as_ref(),
                        workspace.id,
                        revision.clone(),
                        request.expected_task_revision,
                        now,
                    )
                    .await
                    .map_err(|_| Error::StaleContext)?;
                Some(MatrixDispositionVerification::LegacyV1 {
                    composition,
                    verification: verification.ok_or(Error::StaleContext)?,
                })
            }
        } else {
            None
        };
        if selected {
            let verification = validated_snapshot.as_ref().ok_or(Error::StaleContext)?;
            selected_snapshot_current(
                &opportunity,
                verification.composition(),
                verification.record_digest(),
                matches!(
                    verification,
                    MatrixDispositionVerification::ContextV2 { .. }
                ),
            )?;
            let choice_set = revision
                .choice_set
                .as_ref()
                .ok_or(Error::InvalidArguments)?;
            if opportunity.material_digest
                != verification
                    .disposition_digest(&revision.input, choice_set)
                    .map_err(|_| Error::StaleContext)?
            {
                return Err(Error::StaleContext);
            }
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
                let verification = validated_snapshot.as_ref().ok_or(Error::StaleContext)?;
                let fresh = match verification {
                    MatrixDispositionVerification::LegacyV1 {
                        composition,
                        verification,
                    } => crate::MatrixProviderRequest::new_verified(
                        revision.clone(),
                        composition.clone(),
                        verification,
                        stored.record.provider_profile_ref.clone(),
                        stored.record.model_configuration.clone(),
                    ),
                    MatrixDispositionVerification::ContextV2 {
                        binding,
                        composition,
                        record,
                    } => crate::MatrixProviderRequest::new_context_verified(
                        revision.clone(),
                        composition,
                        record,
                        binding.snapshot_id,
                        stored.record.provider_profile_ref.clone(),
                        stored.record.model_configuration.clone(),
                    ),
                }
                .map_err(|_| Error::StaleContext)?;
                let advice = super::matrix_tasks::current_public_matrix_advice(
                    &opportunity,
                    &stored,
                    &config,
                    Some(fresh.binding()),
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
                if requires_current_verification(request) {
                    validated_snapshot.as_ref()
                } else {
                    None
                },
            )
            .await?;
        tx.commit().await?;
        Ok(saved)
    }

    pub async fn get_matrix_disposition_by_request(
        &self,
        context: &RequestContext,
        task_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<MatrixDispositionRecord>> {
        if task_id.is_nil() || request_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, workspace, role) = self.candidate_read_transaction(context).await?;
        // The helper authenticated this exact host/session and validated the
        // workspace membership and revocation state. Identity is never a caller field.
        let reader = if role == PrincipalRole::Owner {
            let session = tx
                .session(context.auth.host_id, &context.native_session_id)
                .await?
                .ok_or(Error::WorkspaceNotOpen)?;
            Some((tx.session_principal(session.id).await?, session.id))
        } else {
            None
        };
        let found = tx
            .matrix_disposition_by_request(workspace.id, request_id)
            .await?;
        let visible =
            found.filter(|saved| disposition_visible_to_reader(saved, task_id, role, reader));
        tx.commit().await?;
        Ok(visible)
    }
}

fn requires_current_verification(request: &RecordMatrixDisposition) -> bool {
    matches!(
        &request.decision,
        MatrixDispositionDecision::Selected { .. }
    ) || request.basis == MatrixDispositionBasis::AfterAdvice
}

fn disposition_visible_to_reader(
    saved: &MatrixDispositionRecord,
    task_id: Uuid,
    role: PrincipalRole,
    reader: Option<(Uuid, Uuid)>,
) -> bool {
    saved.request.task_id == task_id
        && match role {
            PrincipalRole::Verifier => true,
            PrincipalRole::Owner => reader.is_some_and(|(principal, session)| {
                saved.recorded_by_principal_id == principal
                    && saved.recorded_by_session_id == session
            }),
        }
}

fn selected_snapshot_current(
    opportunity: &AdvisoryOpportunity,
    composition: &EngineeringMatrixComposition,
    verification_digest: &str,
    context_v2: bool,
) -> Result<()> {
    if matches!(
        opportunity.primary_reason,
        AdvisoryReason::MatrixEvidenceUnresolved | AdvisoryReason::MatrixSourceUnverified
    ) || opportunity.matrix_verification_digest.as_deref() != Some(verification_digest)
        || (!context_v2
            && composition.source_verification_status
                != MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported)
        || if context_v2 {
            !composition.unresolved_evidence.is_empty()
        } else {
            !composition.is_resolved()
        }
        || composition.catalogue_version != ENGINEERING_MATRIX_CATALOGUE_VERSION
        || !composition
            .mandatory_cards
            .iter()
            .any(|card| card.id == "EM02-SCOPE@0.1")
    {
        return Err(Error::StaleContext);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
