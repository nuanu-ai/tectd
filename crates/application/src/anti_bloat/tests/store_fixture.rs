#[async_trait]
impl AntiBloatStore for FakeStore {
    async fn provider_profile_matches(&mut self, _: Uuid, profile: &str) -> Result<bool> {
        Ok(self.selected_profile.as_deref() == Some(profile))
    }
    async fn record_preflight_no_call(&mut self, _: Uuid, reason: AntiBloatNoCall) -> Result<()> {
        assert_eq!(
            self.saved.as_ref().unwrap().state,
            AntiBloatAttemptState::Prepared
        );
        assert!(self.prepared.is_none());
        assert_eq!(self.sends, 0);
        self.saved.as_mut().unwrap().state = AntiBloatAttemptState::NoCall(reason);
        Ok(())
    }
    async fn authorized_budget_policy(
        &mut self,
        _: Uuid,
        _: i64,
    ) -> Result<Option<AdvisoryBudgetPolicy>> {
        Ok(self.policy.clone())
    }
    async fn advisory_mode(&mut self, _: Uuid) -> Result<WorkspaceAdvisoryMode> {
        Ok(self.mode)
    }
    async fn authoritative_input(
        &mut self,
        _: Uuid,
        _: Uuid,
        _: i64,
    ) -> Result<Option<AntiBloatInput>> {
        Ok(self.input.clone())
    }
    async fn save_review(
        &mut self,
        record: StoredAntiBloatReview,
    ) -> Result<StoredAntiBloatReview> {
        self.saved = Some(record.clone());
        Ok(record)
    }
    async fn review(&mut self, _: Uuid) -> Result<Option<StoredAntiBloatReview>> {
        Ok(self.saved.clone())
    }
    async fn begin_send(
        &mut self,
        saved: &StoredAntiBloatReview,
        prepared: &AntiBloatPreparedRequest,
        _: &AdvisoryBudgetPolicy,
        _: Option<&str>,
    ) -> Result<Option<AntiBloatSendPermit>> {
        if self.sends != 0 {
            return Ok(None);
        }
        self.sends += 1;
        self.prepared = Some(prepared.clone());
        self.saved.as_mut().unwrap().state = AntiBloatAttemptState::Sending;
        Ok(Some(AntiBloatSendPermit {
            review_id: saved.review_id,
            request: prepared.clone(),
        }))
    }
    async fn mark_send_unknown(&mut self, _: Uuid) -> Result<()> {
        self.saved.as_mut().unwrap().state = AntiBloatAttemptState::SendUnknown;
        Ok(())
    }
    async fn seal_ranked(&mut self, _: Uuid, ranked: &[String]) -> Result<()> {
        if self.fail_finish {
            self.fail_finish = false;
            return Err(Error::StorageUnavailable);
        }
        self.seals += 1;
        self.saved.as_mut().unwrap().state = AntiBloatAttemptState::Ranked(ranked.to_vec());
        Ok(())
    }
    async fn seal_response(
        &mut self,
        permit: &AntiBloatSendPermit,
        observation: &AntiBloatProviderObservation,
        sha256: &str,
    ) -> Result<()> {
        assert_eq!(self.prepared.as_ref(), Some(&permit.request));
        assert_eq!(
            format!("{:x}", sha2::Sha256::digest(&observation.raw)),
            sha256
        );
        self.raw_response = Some(observation.raw.clone());
        self.sealed_observation = Some(observation.clone());
        self.response_sha256 = Some(sha256.into());
        Ok(())
    }
    async fn consume_budget(
        &mut self,
        _: &AntiBloatSendPermit,
        observation: &AntiBloatProviderObservation,
    ) -> Result<bool> {
        if self.fail_consume {
            self.fail_consume = false;
            return Err(Error::StorageUnavailable);
        }
        assert_eq!(
            self.raw_response.as_deref(),
            Some(observation.raw.as_slice())
        );
        if let Some(saved) = &self.consumed {
            assert_eq!(saved, observation);
        } else {
            self.consumption_count += 1;
            self.consumed = Some(observation.clone());
        }
        Ok(observation.input_tokens.is_none()
            || observation.output_tokens.is_none()
            || observation.elapsed_monotonic_ms.is_none()
            || observation.input_tokens.is_some_and(|n| n > 100)
            || observation.output_tokens.is_some_and(|n| n > 100)
            || observation.elapsed_monotonic_ms.is_some_and(|n| n > 1000))
    }
    async fn authorized_sealed_response(
        &mut self,
        permit: &AntiBloatSendPermit,
    ) -> Result<AntiBloatProviderObservation> {
        if self.prepared.as_ref() != Some(&permit.request) {
            return Err(Error::InputConflict);
        }
        let consumed = self.consumed.as_ref().ok_or(Error::InputConflict)?;
        if consumed.input_tokens.is_none()
            || consumed.output_tokens.is_none()
            || consumed.elapsed_monotonic_ms.is_none()
            || consumed.input_tokens.is_some_and(|n| n > 100)
            || consumed.output_tokens.is_some_and(|n| n > 100)
            || consumed.elapsed_monotonic_ms.is_some_and(|n| n > 1000)
        {
            return Err(Error::InputConflict);
        }
        self.sealed_observation.clone().ok_or(Error::InputConflict)
    }
    async fn sealed_response_for_usage(
        &mut self,
        permit: &AntiBloatSendPermit,
    ) -> Result<AntiBloatProviderObservation> {
        if self.prepared.as_ref() != Some(&permit.request)
            || self.saved.as_ref().unwrap().state != AntiBloatAttemptState::Sending
        {
            return Err(Error::InputConflict);
        }
        self.sealed_observation.clone().ok_or(Error::InputConflict)
    }
    async fn saved_sealed_response(
        &mut self,
        review_id: Uuid,
    ) -> Result<Option<crate::AntiBloatSealedResponse>> {
        if self.fail_recovery_read {
            self.fail_recovery_read = false;
            return Err(Error::StorageUnavailable);
        }
        Ok(self
            .sealed_observation
            .clone()
            .map(|observation| crate::AntiBloatSealedResponse {
                permit: AntiBloatSendPermit {
                    review_id,
                    request: self.prepared.clone().unwrap(),
                },
                observation,
            }))
    }
    async fn seal_terminal(
        &mut self,
        permit: &AntiBloatSendPermit,
        state: AntiBloatAttemptState,
    ) -> Result<()> {
        self.sealed_response_for_usage(permit).await?;
        if self.consumed.is_none() {
            return Err(Error::InputConflict);
        }
        if state == AntiBloatAttemptState::ProviderAbstained {
            self.authorized_sealed_response(permit).await?;
        }
        assert!(matches!(
            state,
            AntiBloatAttemptState::ProviderAbstained | AntiBloatAttemptState::InvalidResponse
        ));
        self.saved.as_mut().unwrap().state = state;
        Ok(())
    }
    async fn apply_preserved_delta(
        &mut self,
        authored: &AntiBloatAuthoredDelta,
        input: &AntiBloatInput,
        preservation: &AntiBloatPreservation,
        after: &ResolvedCandidateDraft,
    ) -> Result<AntiBloatApplyReceipt> {
        let review_id = authored.review_id;
        let finding_id = authored.finding_id.as_str();
        let disposition = authored.disposition;
        let delta = &authored.delta;
        if let Some(applied) = &self.applied {
            if applied.review_id == review_id
                && &applied.input == input
                && applied.finding_id == finding_id
                && applied.disposition == disposition
                && &applied.preservation == preservation
                && &applied.delta == delta
                && &applied.after == after
            {
                return Ok(applied.receipt.clone());
            }
            return Err(Error::InputConflict);
        }
        if self.input.as_ref() != Some(input)
            || review_anti_bloat(&Sha256ScopeDigest, input)? != self.saved.as_ref().unwrap().review
        {
            return Err(Error::InputConflict);
        }
        self.applies += 1;
        self.after = Some(after.clone());
        let receipt = AntiBloatApplyReceipt {
            review_id,
            candidate_set_id: delta.candidate_set_id,
            idempotency_key: delta.idempotency_key.clone(),
            caller_request_id: Uuid::from_u128(999),
            from_revision: delta.expected_revision,
            to_revision: delta.expected_revision + 1,
            source_digest: preservation.source_digest.clone(),
            before_material_digest: preservation.before_material_digest.clone(),
            after_material_digest: preservation.after_material_digest.clone(),
        };
        self.applied = Some(AppliedDecision {
            review_id,
            input: input.clone(),
            finding_id: finding_id.into(),
            disposition,
            preservation: preservation.clone(),
            delta: delta.clone(),
            after: after.clone(),
            receipt: receipt.clone(),
        });
        Ok(receipt)
    }
}

