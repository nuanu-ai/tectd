impl MatrixProviderResponse {
    pub fn validate_for(&self, request: &MatrixProviderRequest) -> Result<()> {
        if self.binding != request.binding
            || self.provider_profile_ref != request.provider_profile_ref
            || self.model_configuration != request.model_configuration
            || self.raw_response_payload.is_empty()
            || self.response_payload_sha256
                != format!("{:x}", sha2::Sha256::digest(&self.raw_response_payload))
        {
            return Err(Error::InvalidArguments);
        }
        self.ranking.validate(&request.eligibility)?;
        match (&self.ranking, &self.trial_evidence) {
            (
                MatrixRanking::Ranked {
                    ranked_candidate_ids,
                    ..
                },
                Some(evidence),
            ) if matches!(
                request.binding.verification,
                crate::MatrixVerificationAuthority::ContextV2 { .. }
            ) =>
            {
                evidence.validate_ranked(ranked_candidate_ids)
            }
            (_, Some(_)) => Err(Error::InvalidArguments),
            (_, None) => Ok(()),
        }
    }
}
