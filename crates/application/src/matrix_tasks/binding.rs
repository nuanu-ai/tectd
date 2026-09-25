use super::*;

pub(crate) fn current_public_matrix_advice(
    receipt: &AdvisoryOpportunity,
    stored: &crate::StoredGuardedMatrixAdviceRecord,
    config: &WorkspaceAdvisoryConfig,
    fresh: Option<&crate::MatrixProviderBinding>,
) -> Option<CurrentMatrixAdvice> {
    let record = &stored.record;
    let binding = &record.binding;
    let fresh = fresh?;
    if receipt.state != AdvisoryOpportunityState::Advised
        || receipt.primary_reason != AdvisoryReason::ProviderResponse
        || !receipt.provider_called
        || receipt.id != record.opportunity_id
        || receipt.target_id != Some(binding.task_id)
        || receipt.matrix_task_revision != Some(binding.task_revision)
        || receipt.matrix_choice_set_digest.as_deref() != Some(binding.choice_set_digest.as_str())
        || receipt.matrix_verification_digest.as_deref() != binding.verification_digest.as_deref()
        || receipt.material_digest != binding.evaluation_digest
        || receipt.config_revision != config.revision
        || config.mode == WorkspaceAdvisoryMode::Disabled
        || config.provider_profile_ref.as_ref() != Some(&record.provider_profile_ref)
        || config.model_configuration.as_ref() != Some(&record.model_configuration)
        || fresh != binding
        || !matches!(
            record.outcome,
            crate::GuardedMatrixAdviceOutcome::Ranked { .. }
                | crate::GuardedMatrixAdviceOutcome::Abstained { .. }
        )
    {
        return None;
    }
    Some(CurrentMatrixAdvice {
        advice_id: stored.advice_id,
        dispatch_id: record.dispatch_id,
        task_revision: binding.task_revision,
        input_digest: binding.input_digest.clone(),
        choice_set_id: binding.choice_set_id.clone(),
        choice_set_version: binding.choice_set_version,
        choice_set_digest: binding.choice_set_digest.clone(),
        evaluation_digest: binding.evaluation_digest.clone(),
        verification_digest: binding.verification_digest.clone()?,
        provider_profile_ref: record.provider_profile_ref.clone(),
        model_configuration: record.model_configuration.clone(),
        response_payload_sha256: record.response_payload_sha256.clone(),
        advice_digest: record.advice_digest.clone(),
        outcome: record.outcome.clone(),
    })
}

pub(super) fn valid_advisory_request_key(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.contains('\0') && value.trim() == value
}

pub(super) fn matrix_advisory_receipt_matches(
    receipt: &AdvisoryOpportunity,
    workspace_id: Uuid,
    task_id: Uuid,
    request_key: &str,
) -> bool {
    receipt.workspace_id == workspace_id
        && receipt.capability == AdvisoryCapability::EngineeringProfile
        && receipt.decision_point == AdvisoryDecisionPoint::EngineeringProfileBeforeSelection
        && receipt.target_kind == "matrix_task"
        && receipt.target_id == Some(task_id)
        && receipt
            .matrix_task_revision
            .is_some_and(|revision| revision > 0)
        && receipt.matrix_task_revision == receipt.work_revision
        && receipt.workflow_occurrence_key == request_key
}

pub(super) fn matrix_advisory_replay_matches(
    existing: &AdvisoryOpportunity,
    request: &RequestEngineeringAdvisory,
    session_id: Uuid,
    actor_id: Uuid,
) -> bool {
    existing.capability == AdvisoryCapability::EngineeringProfile
        && existing.decision_point == AdvisoryDecisionPoint::EngineeringProfileBeforeSelection
        && existing.target_kind == "matrix_task"
        && existing.target_id == Some(request.task_id)
        && existing.matrix_task_revision == Some(request.expected_task_revision)
        && existing.work_revision == Some(request.expected_task_revision)
        && existing.workflow_occurrence_key == request.request_key
        && existing.session_id == session_id
        && existing.authorized_actor_id == actor_id
        && existing.session_preference == request.session_preference
        && existing.request_preference == request.request_preference
        && matches!(
            existing.state,
            AdvisoryOpportunityState::NoCall
                | AdvisoryOpportunityState::Prepared
                | AdvisoryOpportunityState::AwaitingResponse
                | AdvisoryOpportunityState::Advised
                | AdvisoryOpportunityState::Failed
                | AdvisoryOpportunityState::Invalidated
                | AdvisoryOpportunityState::Unresolved
        )
}

pub(crate) fn matrix_advisory_opportunity_input(
    revision: &MatrixTaskRevision,
    request: &RequestEngineeringAdvisory,
    config: &WorkspaceAdvisoryConfig,
    session_id: Uuid,
    actor_id: Uuid,
) -> Result<AdvisoryOpportunityInput> {
    let composition = compose_current_revision(revision.clone(), request.expected_task_revision)?;
    let eligibility = revision
        .choice_set
        .as_ref()
        .map(|choice| choice.validate(&revision.input))
        .transpose()?
        .unwrap_or(MatrixAdviceEligibility::NotApplicable);
    let reason = if config.mode == WorkspaceAdvisoryMode::Disabled {
        AdvisoryReason::WorkspaceDisabled
    } else if request.session_preference == AdvisoryRequestPreference::Skip {
        AdvisoryReason::SessionSkip
    } else if request.request_preference == AdvisoryRequestPreference::Skip {
        AdvisoryReason::RequestSkip
    } else if eligibility == MatrixAdviceEligibility::NotApplicable {
        AdvisoryReason::ChoiceSetNotApplicable
    } else if !composition.unresolved_evidence.is_empty() {
        AdvisoryReason::MatrixEvidenceUnresolved
    } else if composition.source_verification_status
        != MatrixSourceVerificationStatus::VerifiedByCaller
    {
        AdvisoryReason::MatrixSourceUnverified
    } else {
        AdvisoryReason::CapabilityUnavailable
    };
    // Keep the original no-call material shape: saved receipts from earlier
    // releases include this nested Option even when no evaluation exists.
    let evaluation_digest = revision
        .choice_set
        .as_ref()
        .filter(|_| {
            matches!(
                eligibility,
                MatrixAdviceEligibility::EligibleForAdvice { .. }
            )
        })
        .map(|choice| tect_domain::matrix_evaluation_digest(&revision.input, &composition, choice))
        .transpose()?;
    let material = serde_json::to_vec(&(
        "tect.matrix-advisory-opportunity/1",
        revision.task_id,
        revision.revision,
        &revision.input_digest,
        &revision.choice_set_digest,
        &evaluation_digest,
        &composition,
        config,
        request.session_preference,
        request.request_preference,
        ADVISORY_POLICY_VERSION,
    ))
    .map_err(|_| Error::InternalInvariant)?;
    let input = AdvisoryOpportunityInput {
        session_id,
        authorized_actor_id: actor_id,
        capability: AdvisoryCapability::EngineeringProfile,
        decision_point: AdvisoryDecisionPoint::EngineeringProfileBeforeSelection,
        decision_point_version: ADVISORY_DECISION_POINT_VERSION,
        workflow_occurrence_key: request.request_key.clone(),
        target_kind: "matrix_task".into(),
        target_id: Some(revision.task_id),
        work_revision: Some(revision.revision),
        matrix_task_revision: Some(revision.revision),
        matrix_choice_set_digest: revision.choice_set_digest.clone(),
        matrix_verification_digest: None,
        source_ref: None,
        parent_opportunity_id: None,
        session_preference: request.session_preference,
        request_preference: request.request_preference,
        config_revision: config.revision,
        material_digest: format!("{:x}", Sha256::digest(material)),
        state: AdvisoryOpportunityState::NoCall,
        primary_reason: reason,
    };
    input.validate()?;
    Ok(input)
}

pub(crate) fn compose_current_revision(
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

/// Read only the newest immutable record for the exact current revision. An
/// expired or revoked latest record does not fall back to an older one.
pub(crate) async fn compose_current_revision_with_verification(
    store: Option<&mut dyn MatrixVerificationStore>,
    validator: &dyn MatrixEvidenceValidator,
    workspace_id: Uuid,
    revision: MatrixTaskRevision,
    expected_task_revision: i64,
    now: i64,
) -> Result<(EngineeringMatrixComposition, Option<String>)> {
    let (composition, verification) = compose_current_revision_with_validated_verification(
        store,
        validator,
        workspace_id,
        revision,
        expected_task_revision,
        now,
    )
    .await?;
    Ok((
        composition,
        verification.map(|v| v.record_digest().to_owned()),
    ))
}

/// The positive request seam retains the validated token only after the
/// latest saved record and every evidence binding pass current revalidation.
pub(crate) async fn compose_current_revision_with_validated_verification(
    store: Option<&mut dyn MatrixVerificationStore>,
    validator: &dyn MatrixEvidenceValidator,
    workspace_id: Uuid,
    revision: MatrixTaskRevision,
    expected_task_revision: i64,
    now: i64,
) -> Result<(
    EngineeringMatrixComposition,
    Option<crate::RevalidatedMatrixVerification>,
)> {
    let provisional = compose_current_revision(revision.clone(), expected_task_revision)?;
    let Some(store) = store else {
        return Ok((provisional, None));
    };
    let Some(record) = store
        .matrix_verification_for_revision(
            workspace_id,
            revision.task_id,
            revision.revision,
            &revision.input_digest,
        )
        .await?
    else {
        return Ok((provisional, None));
    };
    if record.owner_principal != revision.recorded_by_principal_id.to_string()
        || record.verifier_principal == record.owner_principal
        || record.policy_version != validator.policy_version()
        || record.input_digest != revision.input_digest
    {
        return Ok((provisional, None));
    }
    let Ok(validated) = evaluate_matrix_verification(
        &revision.task_id.to_string(),
        &revision.revision.to_string(),
        &revision.input,
        &record,
        now,
    ) else {
        return Ok((provisional, None));
    };
    let Ok(required) = required_matrix_facts(&revision.input) else {
        return Ok((provisional, None));
    };
    for fact in &required {
        let Some(binding) = record
            .bindings
            .iter()
            .find(|binding| binding.fact_path == fact.path)
        else {
            return Ok((provisional, None));
        };
        if validator
            .revalidate(
                workspace_id,
                revision.task_id,
                revision.revision,
                fact,
                binding,
                now,
            )
            .await
            .is_err()
        {
            return Ok((provisional, None));
        }
    }
    let reported = OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
        revision.task_id.to_string(),
        revision.revision.to_string(),
        revision.input,
    )?;
    Ok((
        compose_independently_verified_owner_matrix(&reported, &validated)?,
        Some(crate::RevalidatedMatrixVerification::from_revalidated(
            validated,
        )),
    ))
}

/// Rebuild the exact positive request from the current task head and newest
/// independently revalidated evidence. A changed or missing binding is stale.
pub(crate) async fn matrix_request_still_current(
    store: Option<&mut dyn MatrixVerificationStore>,
    validator: &dyn MatrixEvidenceValidator,
    workspace_id: Uuid,
    current: Option<MatrixTaskRevision>,
    expected: &crate::MatrixProviderRequest,
) -> bool {
    let Some(current) = current else { return false };
    let Ok(now) = crate::matrix_verification::current_epoch_seconds() else {
        return false;
    };
    let Ok((composition, Some(verification))) =
        compose_current_revision_with_validated_verification(
            store,
            validator,
            workspace_id,
            current.clone(),
            expected.revision().revision,
            now,
        )
        .await
    else {
        return false;
    };
    let Ok(fresh) = crate::MatrixProviderRequest::new_verified(
        current,
        composition,
        &verification,
        expected.provider_profile_ref().clone(),
        expected.model_configuration().clone(),
    ) else {
        return false;
    };
    &fresh == expected
}
