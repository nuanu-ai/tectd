use super::*;

/// All eight rev4 SliceRun kinds receive a recorded eligibility outcome.
pub fn build_pipeline_recommendation_manifest(
    source: &PipelineRecommendationSource,
) -> Result<PipelineRecommendationManifest> {
    let SliceCandidateNode::Work {
        id,
        revision,
        pipeline,
        ..
    } = &source.work
    else {
        return Err(Error::InvalidArguments);
    };
    if id.is_nil() || *revision < 1 || *revision != source.current_work_revision {
        return Err(Error::StaleRevision);
    }
    let matrix = &source.matrix;
    if matrix.composition.task_id.trim().is_empty()
        || matrix.composition.task_revision.trim().is_empty()
        || matrix.composition.task_revision != matrix.current_task_revision
        || matrix.selected_choice_id.trim().is_empty()
        || matrix.selected_choice_id != matrix.current_selected_choice_id
        || !valid_sha256(&matrix.choice_set_digest)
        || !valid_sha256(&matrix.verification_digest)
        || matrix.choice_set_digest != matrix.current_choice_set_digest
        || matrix.verification_digest != matrix.current_verification_digest
        || (matrix.authority.is_none() && !matrix.composition.is_resolved())
    {
        return Err(Error::StaleContext);
    }
    if matrix.authority.is_none()
        && !matches!(
            matrix.composition.source_verification_status,
            MatrixSourceVerificationStatus::VerifiedByCaller
                | MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported
        )
    {
        return Err(Error::StaleContext);
    }
    matrix.input.validate()?;
    if matrix.choice_set.task_id != matrix.composition.task_id
        || matrix.choice_set.task_revision != matrix.composition.task_revision
        || matrix.choice_set.canonical_digest(&matrix.input)? != matrix.choice_set_digest
        || !matrix
            .choice_set
            .candidates
            .iter()
            .any(|candidate| candidate.candidate_id == matrix.selected_choice_id)
    {
        return Err(Error::StaleContext);
    }
    let expected_composition = match matrix.composition.source_verification_status {
        MatrixSourceVerificationStatus::VerifiedByCaller => compose_engineering_matrix(
            &VerifiedEngineeringMatrixFacts::bind_caller_verified_task_revision(
                matrix.composition.task_id.clone(),
                matrix.composition.task_revision.clone(),
                matrix.input.clone(),
            )?,
        ),
        MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported => {
            let mut expected = compose_owner_reported_engineering_matrix(
                &OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
                    matrix.composition.task_id.clone(),
                    matrix.composition.task_revision.clone(),
                    matrix.input.clone(),
                )?,
            );
            expected.source_verification_status =
                MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported;
            expected
        }
        MatrixSourceVerificationStatus::OwnerReportedPendingIndependentVerification => {
            if matrix.authority.is_none() || !matrix.composition.unresolved_evidence.is_empty() {
                return Err(Error::StaleContext);
            }
            compose_owner_reported_engineering_matrix(
                &OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
                    matrix.composition.task_id.clone(),
                    matrix.composition.task_revision.clone(),
                    matrix.input.clone(),
                )?,
            )
        }
    };
    if let Some(authority) = &matrix.authority {
        authority.validate()?;
        if matrix.composition.source_verification_status
            != MatrixSourceVerificationStatus::OwnerReportedPendingIndependentVerification
            || authority.operating_verification_digest != matrix.verification_digest
        {
            return Err(Error::StaleContext);
        }
    }
    if matrix.composition != expected_composition {
        return Err(Error::StaleContext);
    }
    let input_digest = matrix_input_digest(&matrix.input)?;
    let selected_candidate = matrix
        .choice_set
        .candidates
        .iter()
        .find(|candidate| candidate.candidate_id == matrix.selected_choice_id)
        .ok_or(Error::StaleContext)?;
    let selected_candidate_digest = digest_json(selected_candidate)?;
    let mandatory_card_ids = matrix
        .composition
        .mandatory_cards
        .iter()
        .map(|card| card.id.to_string())
        .collect::<BTreeSet<_>>();
    let saved_card_ids = matrix
        .saved_mandatory_card_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if mandatory_card_ids.is_empty()
        || saved_card_ids.len() != matrix.saved_mandatory_card_ids.len()
        || !mandatory_card_ids.is_subset(&saved_card_ids)
    {
        return Err(Error::InvalidArguments);
    }
    if source.catalogue.revision != PIPELINE_RECOMMENDATION_CATALOGUE_REVISION {
        return Err(Error::StaleContext);
    }
    source.catalogue.validate()?;
    let catalogue_kinds = source
        .catalogue
        .entries
        .iter()
        .map(|entry| entry.kind)
        .collect::<BTreeSet<_>>();
    if !PipelineKind::CURRENT_SLICE_RUN_KINDS
        .iter()
        .all(|kind| catalogue_kinds.contains(kind))
    {
        return Err(Error::StaleContext);
    }
    let mut definitions = BTreeMap::new();
    for definition in &source.definitions {
        if definitions.insert(definition.kind, definition).is_some() {
            return Err(Error::InvalidArguments);
        }
    }
    let mut options = Vec::new();
    let mut excluded = Vec::new();
    for kind in PipelineKind::CURRENT_SLICE_RUN_KINDS {
        let entry = source
            .catalogue
            .entries
            .iter()
            .find(|entry| entry.kind == kind)
            .ok_or(Error::StaleContext)?;
        if !entry.executable || entry.execution_owner != PipelineExecutionOwner::SlicePipelineRun {
            excluded.push(PipelineExcludedKind {
                kind,
                reason: PipelineExclusionReason::UnavailableDefinition,
            });
            continue;
        }
        let Some(definition) = definitions.get(&kind) else {
            excluded.push(PipelineExcludedKind {
                kind,
                reason: PipelineExclusionReason::UnavailableDefinition,
            });
            continue;
        };
        if definition.validate().is_err()
            || definition.default_mode
                != entry
                    .default_delivery_mode
                    .unwrap_or(definition.default_mode)
            || definition.allowed_modes != entry.allowed_delivery_modes
        {
            excluded.push(PipelineExcludedKind {
                kind,
                reason: PipelineExclusionReason::UnavailableDefinition,
            });
            continue;
        }
        let verification_plan = PipelineVerificationPlan::from_definition(definition)?;
        let option = PipelineRecommendationOption {
            id: PipelineRecommendationOption::pair_id(kind, &verification_plan.id),
            kind,
            definition_version: definition.version.clone(),
            definition_digest: definition.digest.clone(),
            completion_contract: definition.completion_contract.clone(),
            forbidden_claims: definition.forbidden_claims.clone(),
            verification_plan,
        };
        let context = PipelineCompatibilityContext {
            task_id: &matrix.composition.task_id,
            task_revision: &matrix.composition.task_revision,
            catalogue_revision: &source.catalogue.revision,
            input: &matrix.input,
            input_digest: &input_digest,
            selected_candidate_id: &matrix.selected_choice_id,
            mandatory_cards: &mandatory_card_ids,
        };
        if let Some(reason) = source
            .compatibility_policy
            .reason_for(kind, &context, &option)
        {
            excluded.push(PipelineExcludedKind { kind, reason });
        } else {
            options.push(option);
        }
    }
    if source
        .evidence_refs
        .iter()
        .any(|value| value.trim().is_empty())
        || source.evidence_refs.iter().collect::<BTreeSet<_>>().len() != source.evidence_refs.len()
    {
        return Err(Error::InvalidArguments);
    }
    let mut manifest = PipelineRecommendationManifest {
        schema: if matrix.authority.is_some() {
            PIPELINE_RECOMMENDATION_CONTEXT_SCHEMA
        } else {
            PIPELINE_RECOMMENDATION_SCHEMA
        }
        .into(),
        work_id: *id,
        work_revision: *revision,
        matrix_task_id: matrix.composition.task_id.clone(),
        matrix_task_revision: matrix.composition.task_revision.clone(),
        selected_choice_id: matrix.selected_choice_id.clone(),
        matrix_choice_set_digest: matrix.choice_set_digest.clone(),
        matrix_verification_digest: matrix.verification_digest.clone(),
        matrix_authority: matrix.authority.clone(),
        matrix_input_digest: input_digest,
        selected_candidate_digest,
        compatibility_policy_digest: source.compatibility_policy.digest()?,
        mandatory_card_ids: mandatory_card_ids.into_iter().collect(),
        deterministic_kind: *pipeline,
        deterministic_option_id: options
            .iter()
            .find(|option| option.kind == *pipeline)
            .map(|option| option.id.clone()),
        catalogue_revision: source.catalogue.revision.clone(),
        catalogue_digest: source.catalogue.digest.clone(),
        options,
        excluded,
        evidence_refs: source
            .evidence_refs
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        digest: String::new(),
    };
    manifest.digest = digest_json(&manifest)?;
    Ok(manifest)
}
