impl MatrixProviderRequest {
    /// Build only from the accepted revision and its complete composition.
    /// Domain contracts remain the authority for eligibility and evaluation.
    pub fn new(
        revision: MatrixTaskRevision,
        composition: EngineeringMatrixComposition,
        provider_profile_ref: AdvisoryProviderProfileRef,
        model_configuration: AdvisoryModelConfiguration,
    ) -> Result<Self> {
        provider_profile_ref.validate()?;
        model_configuration.validate()?;
        if revision.task_id.is_nil() || revision.revision < 1 {
            return Err(Error::InvalidArguments);
        }
        validate_saved_revision_source_provenance(&composition)?;
        let choice_set = revision
            .choice_set
            .as_ref()
            .ok_or(Error::InvalidArguments)?;
        if choice_set.task_id != revision.task_id.to_string()
            || choice_set.task_revision != revision.revision.to_string()
        {
            return Err(Error::StaleRevision);
        }
        let input = serde_json::to_value(&revision.input).map_err(|_| Error::InvalidArguments)?;
        let input_digest = canonical_matrix_input_digest(&input)?;
        let choice_set_digest = choice_set.canonical_digest(&revision.input)?;
        if revision.input_digest != input_digest
            || revision.choice_set_digest.as_deref() != Some(choice_set_digest.as_str())
        {
            return Err(Error::InvalidArguments);
        }
        let eligibility = choice_set.validate(&revision.input)?;
        if !matches!(
            &eligibility,
            MatrixAdviceEligibility::EligibleForAdvice { .. }
        ) {
            return Err(Error::InvalidArguments);
        }
        let evaluation_digest =
            matrix_evaluation_digest(&revision.input, &composition, choice_set)?
                .ok_or(Error::InvalidArguments)?;
        let binding = MatrixProviderBinding {
            task_id: revision.task_id,
            task_revision: revision.revision,
            input_digest,
            choice_set_id: choice_set.choice_set_id.clone(),
            choice_set_version: choice_set.version,
            choice_set_digest,
            evaluation_digest,
            verification_digest: None,
        };
        Ok(Self {
            binding,
            revision,
            composition,
            provider_profile_ref,
            model_configuration,
            eligibility,
        })
    }

    /// Construct the v2 positive binding from the application's validated
    /// verification token. The token is tied to the exact input and revision.
    pub fn new_verified(
        revision: MatrixTaskRevision,
        composition: EngineeringMatrixComposition,
        verification: &RevalidatedMatrixVerification,
        provider_profile_ref: AdvisoryProviderProfileRef,
        model_configuration: AdvisoryModelConfiguration,
    ) -> Result<Self> {
        let mut request = Self::new_for_verified_composition(
            revision,
            composition,
            provider_profile_ref,
            model_configuration,
        )?;
        let choice_set = request
            .revision
            .choice_set
            .as_ref()
            .ok_or(Error::InvalidArguments)?;
        request.binding.evaluation_digest = tect_domain::matrix_verified_evaluation_digest(
            &request.revision.input,
            &request.composition,
            choice_set,
            &verification.validated,
        )?;
        request.binding.verification_digest = Some(verification.record_digest().to_owned());
        Ok(request)
    }

    fn new_for_verified_composition(
        revision: MatrixTaskRevision,
        composition: EngineeringMatrixComposition,
        provider_profile_ref: AdvisoryProviderProfileRef,
        model_configuration: AdvisoryModelConfiguration,
    ) -> Result<Self> {
        // Reuse all revision, choice-set, digest and eligibility guards while
        // changing only the provenance status accepted by this constructor.
        if composition.source_verification_status
            != MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported
        {
            return Err(Error::InvalidArguments);
        }
        let mut provisional = composition.clone();
        provisional.source_verification_status =
            MatrixSourceVerificationStatus::OwnerReportedPendingIndependentVerification;
        let mut request = Self::new(
            revision,
            provisional,
            provider_profile_ref,
            model_configuration,
        )?;
        request.composition = composition;
        Ok(request)
    }

    pub fn binding(&self) -> &MatrixProviderBinding {
        &self.binding
    }

    pub fn revision(&self) -> &MatrixTaskRevision {
        &self.revision
    }

    pub fn composition(&self) -> &EngineeringMatrixComposition {
        &self.composition
    }

    pub fn provider_profile_ref(&self) -> &AdvisoryProviderProfileRef {
        &self.provider_profile_ref
    }

    pub fn model_configuration(&self) -> &AdvisoryModelConfiguration {
        &self.model_configuration
    }

    pub fn eligibility(&self) -> &MatrixAdviceEligibility {
        &self.eligibility
    }
}

fn validate_saved_revision_source_provenance(
    composition: &EngineeringMatrixComposition,
) -> Result<()> {
    if composition.source_verification_status
        != MatrixSourceVerificationStatus::OwnerReportedPendingIndependentVerification
    {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

#[cfg(test)]
mod matrix_provider_request_tests {
    use super::*;

    fn composition(status: MatrixSourceVerificationStatus) -> EngineeringMatrixComposition {
        EngineeringMatrixComposition {
            catalogue_version: "EM02-INITIAL@0.1",
            task_id: "task-1".into(),
            task_revision: "1".into(),
            source_verification_status: status,
            mandatory_cards: Vec::new(),
            unresolved_evidence: Vec::new(),
        }
    }

    #[test]
    fn saved_revision_rejects_caller_verified_composition() {
        assert_eq!(
            validate_saved_revision_source_provenance(&composition(
                MatrixSourceVerificationStatus::VerifiedByCaller,
            )),
            Err(Error::InvalidArguments)
        );
        assert_eq!(
            validate_saved_revision_source_provenance(&composition(
                MatrixSourceVerificationStatus::OwnerReportedPendingIndependentVerification,
            )),
            Ok(())
        );
    }
}
