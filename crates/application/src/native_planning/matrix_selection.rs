use crate::{MatrixPlanningMappedNode, MatrixProviderRequest, UnitOfWork, WorkspaceService};
use tect_domain::{
    AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryOpportunityState, AdvisoryReason,
    ENGINEERING_MATRIX_CATALOGUE_VERSION, Error, MatrixDispositionBasis, MatrixDispositionDecision,
    MatrixPlanningSelection, MatrixSourceVerificationStatus, Result, SliceCandidateContext,
    SliceCandidateDraft, SliceCandidateDraftNode, SliceCandidateNode,
};
use uuid::Uuid;

/// Resolve caller-attributed draft positions to the exact nodes saved by the
/// native operation. The native resolver currently preserves submitted order.
pub(super) fn resolve_selected_matrix_nodes(
    selection: &MatrixPlanningSelection,
    draft: &SliceCandidateDraft,
    saved: &SliceCandidateContext,
) -> Result<Vec<MatrixPlanningMappedNode>> {
    let nodes = &saved.draft.as_ref().ok_or(Error::InternalInvariant)?.nodes;
    if nodes.len() != draft.nodes.len()
        || draft.nodes.iter().zip(nodes).any(|(input, output)| {
            !matches!(
                (input, output),
                (
                    SliceCandidateDraftNode::Work { .. },
                    SliceCandidateNode::Work { .. }
                ) | (
                    SliceCandidateDraftNode::Decision { .. },
                    SliceCandidateNode::Decision { .. }
                )
            )
        })
    {
        return Err(Error::InternalInvariant);
    }
    selection
        .mapped_draft_node_indices
        .iter()
        .map(|&draft_index| {
            let input = draft
                .nodes
                .get(draft_index)
                .ok_or(Error::InvalidArguments)?;
            let output = nodes.get(draft_index).ok_or(Error::InternalInvariant)?;
            let identity = match input {
                SliceCandidateDraftNode::Work { identity, .. }
                | SliceCandidateDraftNode::Decision { identity, .. } => identity,
            };
            if identity.candidate_id.is_some_and(|id| id != output.id())
                || identity.revision.is_some_and(|revision| {
                    output.revision() != revision
                        && revision.checked_add(1) != Some(output.revision())
                })
                || output.id().is_nil()
                || output.revision() < 1
            {
                return Err(Error::InternalInvariant);
            }
            Ok(MatrixPlanningMappedNode {
                draft_index,
                node_id: output.id(),
                node_revision: output.revision(),
            })
        })
        .collect()
}

/// Recheck every Matrix binding inside the same write unit of work, before the
/// native planning mutation. These digests are server-derived for the link.
pub(super) async fn validate_selected_matrix_plan(
    service: &WorkspaceService,
    tx: &mut dyn UnitOfWork,
    workspace_id: Uuid,
    principal_id: Uuid,
    selection: &MatrixPlanningSelection,
) -> Result<(String, String)> {
    selection.validate()?;
    let disposition = tx
        .matrix_planning_selection_store()
        .ok_or(Error::StorageUnavailable)?
        .matrix_disposition_by_id(workspace_id, selection.disposition_id)
        .await?
        .ok_or(Error::NotFound)?;
    if disposition.recorded_by_principal_id != principal_id
        || disposition.request.task_id != selection.task_id
        || disposition.request.expected_task_revision != selection.task_revision
        || disposition.request.expected_input_digest != selection.expected_input_digest
        || disposition.request.expected_choice_set_digest.as_deref()
            != Some(selection.expected_choice_set_digest.as_str())
        || !matches!(
            &disposition.request.decision,
            MatrixDispositionDecision::Selected { selected_choice_id }
                if selected_choice_id == &selection.selected_choice_id
        )
    {
        return Err(Error::StaleContext);
    }
    let revision = tx
        .lock_matrix_task(workspace_id, selection.task_id)
        .await?
        .ok_or(Error::NotFound)?;
    if revision.revision != selection.task_revision
        || revision.input_digest != selection.expected_input_digest
        || revision.choice_set_digest.as_deref()
            != Some(selection.expected_choice_set_digest.as_str())
    {
        return Err(Error::StaleRevision);
    }
    let choice_set = revision.choice_set.as_ref().ok_or(Error::StaleContext)?;
    choice_set.validate(&revision.input)?;
    if !choice_set
        .candidates
        .iter()
        .any(|candidate| candidate.candidate_id == selection.selected_choice_id)
    {
        return Err(Error::StaleContext);
    }
    let (composition, verification) =
        crate::matrix_tasks::compose_current_revision_with_validated_verification(
            tx.matrix_verification_store(),
            service.matrix_evidence_validator.as_ref(),
            workspace_id,
            revision.clone(),
            selection.task_revision,
            crate::matrix_verification::current_epoch_seconds()?,
        )
        .await
        .map_err(|_| Error::StaleContext)?;
    let verification = verification.ok_or(Error::StaleContext)?;
    if verification.record_digest() != selection.expected_verification_digest
        || composition.source_verification_status
            != MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported
        || !composition.is_resolved()
        || composition.catalogue_version != ENGINEERING_MATRIX_CATALOGUE_VERSION
        || !composition
            .mandatory_cards
            .iter()
            .any(|card| card.id == "EM02-SCOPE@0.1")
        || composition.mandatory_cards.iter().any(|card| {
            !matches!(
                card.id,
                "EM02-SCOPE@0.1"
                    | "EM02-PROTECT@0.1"
                    | "EM02-OPERATE@0.1"
                    | "EM02-CAPACITY@0.1"
                    | "EM02-HOTFIX@0.1"
            )
        })
    {
        return Err(Error::StaleContext);
    }
    let evaluation_digest =
        verification.disposition_digest(&revision.input, &composition, choice_set)?;
    let opportunity = tx
        .advisory_opportunity_for_dispatch(workspace_id, disposition.request.opportunity_id)
        .await?;
    if opportunity.workspace_id != workspace_id
        || matches!(
            opportunity.primary_reason,
            AdvisoryReason::MatrixEvidenceUnresolved | AdvisoryReason::MatrixSourceUnverified
        )
        || opportunity.capability != AdvisoryCapability::EngineeringProfile
        || opportunity.decision_point != AdvisoryDecisionPoint::EngineeringProfileBeforeSelection
        || opportunity.target_kind != "matrix_task"
        || opportunity.target_id != Some(selection.task_id)
        || opportunity.matrix_task_revision != Some(selection.task_revision)
        || opportunity.work_revision != Some(selection.task_revision)
        || opportunity.matrix_choice_set_digest.as_deref()
            != Some(selection.expected_choice_set_digest.as_str())
        || opportunity.matrix_verification_digest.as_deref()
            != Some(selection.expected_verification_digest.as_str())
        || opportunity.material_digest != evaluation_digest
        || opportunity.authorized_actor_id != disposition.recorded_by_principal_id
        || opportunity.session_id != disposition.recorded_by_session_id
    {
        return Err(Error::StaleContext);
    }
    match disposition.request.basis {
        MatrixDispositionBasis::AfterAdvice
            if opportunity.state == AdvisoryOpportunityState::Advised =>
        {
            let stored = tx
                .guarded_matrix_advice(workspace_id, opportunity.id)
                .await?
                .ok_or(Error::StaleContext)?;
            let config = tx.advisory_config(workspace_id).await?;
            let fresh = MatrixProviderRequest::new_verified(
                revision,
                composition.clone(),
                &verification,
                stored.record.provider_profile_ref.clone(),
                stored.record.model_configuration.clone(),
            )
            .map_err(|_| Error::StaleContext)?;
            let current = crate::matrix_tasks::current_public_matrix_advice(
                &opportunity,
                &stored,
                &config,
                Some(fresh.binding()),
            )
            .ok_or(Error::StaleContext)?;
            if disposition.request.advice_id != Some(current.advice_id)
                || disposition.request.advice_digest.as_deref()
                    != Some(current.advice_digest.as_str())
            {
                return Err(Error::StaleContext);
            }
        }
        MatrixDispositionBasis::NoCall if opportunity.state == AdvisoryOpportunityState::NoCall => {
        }
        MatrixDispositionBasis::Manual
            if matches!(
                opportunity.state,
                AdvisoryOpportunityState::NoCall
                    | AdvisoryOpportunityState::Failed
                    | AdvisoryOpportunityState::Invalidated
            ) => {}
        _ => return Err(Error::StaleContext),
    }
    Ok((evaluation_digest, composition.catalogue_version.into()))
}
