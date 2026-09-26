use super::*;

impl WorkspaceService {
    async fn anti_bloat_transaction(
        &self,
        context: &RequestContext,
        mode: TransactionMode,
    ) -> Result<(Box<dyn UnitOfWork>, Uuid, Uuid)> {
        let (mut tx, identity) = self.authorized(context, mode).await?;
        if mode == TransactionMode::ReadWrite {
            tx.lock_native_session(identity.host_id, &context.native_session_id)
                .await?;
        }
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        Ok((tx, workspace.id, identity.principal_id))
    }

    pub async fn prepare_anti_bloat(
        &self,
        context: &RequestContext,
        candidate_set_id: Uuid,
        expected_revision: i64,
        preference: AdvisoryRequestPreference,
    ) -> Result<StoredAntiBloatReview> {
        let (mut tx, workspace, actor) = self
            .anti_bloat_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let review = prepare_anti_bloat_review(
            tx.anti_bloat_store().ok_or(Error::StorageUnavailable)?,
            workspace,
            actor,
            candidate_set_id,
            expected_revision,
            preference,
        )
        .await?;
        tx.commit().await?;
        Ok(review)
    }

    pub async fn get_anti_bloat(
        &self,
        context: &RequestContext,
        review_id: Uuid,
    ) -> Result<StoredAntiBloatReview> {
        if review_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, workspace, _) = self
            .anti_bloat_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let review = tx
            .anti_bloat_store()
            .ok_or(Error::StorageUnavailable)?
            .review(review_id)
            .await?
            .ok_or(Error::NotFound)?;
        if review.workspace_id != workspace {
            return Err(Error::Forbidden);
        }
        tx.commit().await?;
        Ok(review)
    }

    pub async fn apply_anti_bloat(
        &self,
        context: &RequestContext,
        authored: &AntiBloatAuthoredDelta,
    ) -> Result<AntiBloatApplyReceipt> {
        let (mut tx, workspace, _) = self
            .anti_bloat_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let store = tx.anti_bloat_store().ok_or(Error::StorageUnavailable)?;
        let saved = store
            .review(authored.review_id)
            .await?
            .ok_or(Error::NotFound)?;
        if saved.workspace_id != workspace {
            return Err(Error::Forbidden);
        }
        let receipt = apply_anti_bloat_delta(store, authored).await?;
        tx.commit().await?;
        Ok(receipt)
    }

    /// The one-use send fence is committed before the provider sees a permit.
    /// Raw response bytes are committed before their interpretation.
    pub async fn run_anti_bloat_once(
        &self,
        context: &RequestContext,
        review_id: Uuid,
    ) -> Result<AntiBloatAttemptState> {
        if review_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let (mut fence, workspace, _) = self
            .anti_bloat_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let saved = fence
            .anti_bloat_store()
            .ok_or(Error::StorageUnavailable)?
            .review(review_id)
            .await?
            .ok_or(Error::NotFound)?;
        if saved.workspace_id != workspace {
            return Err(Error::Forbidden);
        }
        let prepared = prepare_anti_bloat_send(
            fence.anti_bloat_store().ok_or(Error::StorageUnavailable)?,
            self.anti_bloat_provider.as_ref(),
            review_id,
        )
        .await?;
        let Some(permit) = prepared.permit else {
            fence.commit().await?;
            return Ok(prepared.state);
        };

        let observation = match rank_after_committed_fence(
            fence.commit(),
            self.anti_bloat_provider.as_ref(),
            &permit,
        )
        .await?
        {
            Ok(observation) => observation.normalized(),
            Err(_) => {
                let (mut uncertain, _, _) = self
                    .anti_bloat_transaction(context, TransactionMode::ReadWrite)
                    .await?;
                uncertain
                    .anti_bloat_store()
                    .ok_or(Error::StorageUnavailable)?
                    .mark_send_unknown(review_id)
                    .await?;
                uncertain.commit().await?;
                return Ok(AntiBloatAttemptState::SendUnknown);
            }
        };
        let (mut seal, _, _) = self
            .anti_bloat_transaction(context, TransactionMode::ReadWrite)
            .await?;
        seal_anti_bloat_response(
            seal.anti_bloat_store().ok_or(Error::StorageUnavailable)?,
            &permit,
            &observation.raw,
        )
        .await?;
        seal.commit().await?;

        let (mut usage_read, _, _) = self
            .anti_bloat_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let (observation, usage_invalid) = observe_sealed_usage(
            usage_read
                .anti_bloat_store()
                .ok_or(Error::StorageUnavailable)?,
            self.anti_bloat_provider.as_ref(),
            &permit,
            &observation,
        )
        .await?;
        usage_read.commit().await?;

        let (mut consume, _, _) = self
            .anti_bloat_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let exhausted = consume
            .anti_bloat_store()
            .ok_or(Error::StorageUnavailable)?
            .consume_budget(&permit, &observation)
            .await?;
        consume.commit().await?;
        if usage_invalid {
            let (mut invalid, _, _) = self
                .anti_bloat_transaction(context, TransactionMode::ReadWrite)
                .await?;
            invalid
                .anti_bloat_store()
                .ok_or(Error::StorageUnavailable)?
                .seal_terminal(&permit, AntiBloatAttemptState::InvalidResponse)
                .await?;
            invalid.commit().await?;
            return Ok(AntiBloatAttemptState::InvalidResponse);
        }
        if exhausted {
            let (mut uncertain, _, _) = self
                .anti_bloat_transaction(context, TransactionMode::ReadWrite)
                .await?;
            uncertain
                .anti_bloat_store()
                .ok_or(Error::StorageUnavailable)?
                .mark_send_unknown(review_id)
                .await?;
            uncertain.commit().await?;
            return Ok(AntiBloatAttemptState::SendUnknown);
        }

        let (mut finish, _, _) = self
            .anti_bloat_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let outcome = finalize_anti_bloat_response(
            finish.anti_bloat_store().ok_or(Error::StorageUnavailable)?,
            self.anti_bloat_provider.as_ref(),
            &permit,
            &observation.raw,
        )
        .await;
        // Invalid provider output transitions to send_unknown even when the
        // command reports a conflict; commit that terminal audit state.
        finish.commit().await?;
        outcome
    }
}
