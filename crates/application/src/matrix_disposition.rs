use crate::{
    CurrentMatrixAdvice, MatrixDispositionRecord, RecordMatrixDisposition,
    RevalidatedMatrixVerification, TransactionMode, WorkspaceService,
};
use tect_domain::{
    AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryOpportunity, AdvisoryOpportunityState,
    AdvisoryReason, ENGINEERING_MATRIX_CATALOGUE_VERSION, EngineeringMatrixComposition, Error,
    MatrixDispositionBasis, MatrixDispositionDecision, MatrixSourceVerificationStatus,
    RequestContext, Result,
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

        let selected = matches!(
            &request.decision,
            MatrixDispositionDecision::Selected { .. }
        );
        let validated_snapshot = if selected || request.basis == MatrixDispositionBasis::AfterAdvice
        {
            let (composition, verification) =
                super::matrix_tasks::compose_current_revision_with_validated_verification(
                    tx.matrix_verification_store(),
                    self.matrix_evidence_validator.as_ref(),
                    workspace.id,
                    revision.clone(),
                    request.expected_task_revision,
                    crate::matrix_verification::current_epoch_seconds()?,
                )
                .await
                .map_err(|_| Error::StaleContext)?;
            Some((composition, verification.ok_or(Error::StaleContext)?))
        } else {
            None
        };
        if selected {
            let (composition, verification) =
                validated_snapshot.as_ref().ok_or(Error::StaleContext)?;
            selected_snapshot_current(&opportunity, composition, verification.record_digest())?;
            let choice_set = revision
                .choice_set
                .as_ref()
                .ok_or(Error::InvalidArguments)?;
            if opportunity.material_digest
                != verification.disposition_digest(&revision.input, composition, choice_set)?
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
                let (composition, verification) =
                    validated_snapshot.as_ref().ok_or(Error::StaleContext)?;
                let fresh = crate::MatrixProviderRequest::new_verified(
                    revision.clone(),
                    composition.clone(),
                    verification,
                    stored.record.provider_profile_ref.clone(),
                    stored.record.model_configuration.clone(),
                )
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
                if selected {
                    validated_snapshot
                        .as_ref()
                        .map(|(_, verification)| verification)
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
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        let found = tx
            .matrix_disposition_by_request(workspace.id, request_id)
            .await?;
        let visible = found.filter(|saved| {
            saved.request.task_id == task_id
                && saved.recorded_by_principal_id == identity.principal_id
                && saved.recorded_by_session_id == session.id
        });
        tx.commit().await?;
        Ok(visible)
    }
}

fn selected_snapshot_current(
    opportunity: &AdvisoryOpportunity,
    composition: &EngineeringMatrixComposition,
    verification_digest: &str,
) -> Result<()> {
    if matches!(
        opportunity.primary_reason,
        AdvisoryReason::MatrixEvidenceUnresolved | AdvisoryReason::MatrixSourceUnverified
    ) || opportunity.matrix_verification_digest.as_deref() != Some(verification_digest)
        || composition.source_verification_status
            != MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported
        || !composition.is_resolved()
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
mod tests {
    use super::*;
    use tect_domain::{
        AdvisoryRequestPreference, MandatoryMatrixCard, MatrixSourceVerificationStatus,
    };

    fn opportunity() -> AdvisoryOpportunity {
        AdvisoryOpportunity {
            id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            authorized_actor_id: Uuid::new_v4(),
            capability: AdvisoryCapability::EngineeringProfile,
            decision_point: AdvisoryDecisionPoint::EngineeringProfileBeforeSelection,
            decision_point_version: 1,
            workflow_occurrence_key: "skip-verified".into(),
            target_kind: "matrix_task".into(),
            target_id: Some(Uuid::new_v4()),
            work_revision: Some(1),
            matrix_task_revision: Some(1),
            matrix_choice_set_digest: Some("b".repeat(64)),
            matrix_verification_digest: Some("a".repeat(64)),
            source_ref: None,
            session_preference: AdvisoryRequestPreference::Skip,
            request_preference: AdvisoryRequestPreference::UseWorkspace,
            config_revision: 1,
            material_digest: "c".repeat(64),
            state: AdvisoryOpportunityState::NoCall,
            primary_reason: AdvisoryReason::SessionSkip,
            provider_called: false,
        }
    }

    fn composition() -> EngineeringMatrixComposition {
        EngineeringMatrixComposition {
            catalogue_version: ENGINEERING_MATRIX_CATALOGUE_VERSION,
            task_id: "task".into(),
            task_revision: "1".into(),
            source_verification_status:
                MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported,
            mandatory_cards: vec![MandatoryMatrixCard {
                id: "EM02-SCOPE@0.1",
                catalogue_version: ENGINEERING_MATRIX_CATALOGUE_VERSION,
                summary: "Scope",
                body: "Mandatory scope",
            }],
            unresolved_evidence: Vec::new(),
        }
    }

    #[derive(Default)]
    struct FakeDispositionPort {
        inserts: usize,
    }

    impl FakeDispositionPort {
        fn select(
            &mut self,
            receipt: &AdvisoryOpportunity,
            cards: &EngineeringMatrixComposition,
        ) -> Result<()> {
            selected_snapshot_current(receipt, cards, &"a".repeat(64))?;
            self.inserts += 1;
            Ok(())
        }
    }

    #[test]
    fn historical_unverified_no_call_cannot_select_even_after_evidence_improves() {
        let mut port = FakeDispositionPort::default();
        for reason in [
            AdvisoryReason::MatrixEvidenceUnresolved,
            AdvisoryReason::MatrixSourceUnverified,
        ] {
            let mut receipt = opportunity();
            receipt.primary_reason = reason;
            receipt.matrix_verification_digest = None;
            assert_eq!(
                port.select(&receipt, &composition()),
                Err(Error::StaleContext)
            );
        }
        assert_eq!(port.inserts, 0);
    }

    #[test]
    fn verified_optional_advice_skip_can_select_but_lost_cards_or_digest_cannot() {
        let mut port = FakeDispositionPort::default();
        let receipt = opportunity();
        port.select(&receipt, &composition()).unwrap();
        assert_eq!(port.inserts, 1);
        let mut stale = receipt.clone();
        stale.matrix_verification_digest = None;
        assert_eq!(
            port.select(&stale, &composition()),
            Err(Error::StaleContext)
        );
        let mut missing_card = composition();
        missing_card.mandatory_cards.clear();
        assert_eq!(
            port.select(&receipt, &missing_card),
            Err(Error::StaleContext)
        );
        assert_eq!(port.inserts, 1);
    }
}
