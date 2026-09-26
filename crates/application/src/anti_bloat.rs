//! Slice 04 application policy. All source/plan facts come from the store port.
use crate::{
    AntiBloatAttemptState, AntiBloatAuthoredDelta, AntiBloatNoCall, AntiBloatPreparedRequest,
    AntiBloatProviderObservation, AntiBloatRankingProvider, AntiBloatSendPermit, AntiBloatStore,
    Sha256ScopeDigest, StoredAntiBloatReview, TransactionMode, UnitOfWork, WorkspaceService,
};
use sha2::{Digest, Sha256};
use std::future::Future;
use tect_domain::{
    AdvisoryRequestPreference, AntiBloatApplyReceipt, AntiBloatDisposition, Error, RequestContext,
    Result, WorkspaceAdvisoryMode, derive_anti_bloat_delta, review_anti_bloat,
};
use uuid::Uuid;

pub struct AntiBloatApplication<S, P> {
    pub store: S,
    pub provider: P,
}

impl<S: AntiBloatStore, P: AntiBloatRankingProvider> AntiBloatApplication<S, P> {
    /// Saves the exact authoritative source, obligations, dependency digest,
    /// whole candidate graph, and deterministic review at the requested revision.
    pub async fn prepare(
        &mut self,
        workspace_id: Uuid,
        actor_id: Uuid,
        candidate_set_id: Uuid,
        expected_revision: i64,
        preference: AdvisoryRequestPreference,
    ) -> Result<StoredAntiBloatReview> {
        prepare_anti_bloat_review(
            &mut self.store,
            workspace_id,
            actor_id,
            candidate_set_id,
            expected_revision,
            preference,
        )
        .await
    }

    /// The caller must commit this unit of work before giving the permit to a provider.
    pub async fn prepare_send(&mut self, review_id: Uuid) -> Result<PreparedAntiBloatSend> {
        prepare_anti_bloat_send(&mut self.store, &self.provider, review_id).await
    }

    /// Seal raw transport bytes in a separate transaction before interpreting them.
    pub async fn seal_response(&mut self, permit: &AntiBloatSendPermit, raw: &[u8]) -> Result<()> {
        seal_anti_bloat_response(&mut self.store, permit, raw).await
    }

    pub async fn finalize_response(
        &mut self,
        permit: &AntiBloatSendPermit,
        raw: &[u8],
    ) -> Result<AntiBloatAttemptState> {
        finalize_anti_bloat_response(&mut self.store, &self.provider, permit, raw).await
    }

    pub async fn disposition_and_apply(
        &mut self,
        authored: &AntiBloatAuthoredDelta,
    ) -> Result<AntiBloatApplyReceipt> {
        apply_anti_bloat_delta(&mut self.store, authored).await
    }
}

pub async fn prepare_anti_bloat_review(
    store: &mut dyn AntiBloatStore,
    workspace_id: Uuid,
    actor_id: Uuid,
    candidate_set_id: Uuid,
    expected_revision: i64,
    preference: AdvisoryRequestPreference,
) -> Result<StoredAntiBloatReview> {
    if workspace_id.is_nil()
        || actor_id.is_nil()
        || candidate_set_id.is_nil()
        || expected_revision < 1
    {
        return Err(Error::InvalidArguments);
    }
    let mode = store.advisory_mode(workspace_id).await?;
    let input = store
        .authoritative_input(workspace_id, candidate_set_id, expected_revision)
        .await?
        .ok_or(Error::NotFound)?;
    if input.manifest.source.candidate_set_id != candidate_set_id
        || input.selected_revision != expected_revision
    {
        return Err(Error::InputConflict);
    }
    let review = review_anti_bloat(&Sha256ScopeDigest, &input)?;
    let state = if mode == WorkspaceAdvisoryMode::Disabled {
        AntiBloatAttemptState::NoCall(AntiBloatNoCall::Disabled)
    } else if preference == AdvisoryRequestPreference::Skip {
        AntiBloatAttemptState::NoCall(AntiBloatNoCall::Skipped)
    } else if !review.findings.iter().any(|finding| finding.rankable) {
        AntiBloatAttemptState::NoCall(AntiBloatNoCall::NoEligibleFindings)
    } else {
        AntiBloatAttemptState::Prepared
    };
    store
        .save_review(StoredAntiBloatReview {
            review_id: Uuid::new_v4(),
            workspace_id,
            actor_id,
            input,
            review,
            state,
        })
        .await
}

/// Derives the complete post-delta graph from the frozen review. The store
/// returns an exact prior receipt or rechecks current source and CAS in its
/// transaction before applying a new decision.
pub async fn apply_anti_bloat_delta(
    store: &mut dyn AntiBloatStore,
    authored: &AntiBloatAuthoredDelta,
) -> Result<AntiBloatApplyReceipt> {
    let saved = store
        .review(authored.review_id)
        .await?
        .ok_or(Error::NotFound)?;
    if saved.review_id != authored.review_id {
        return Err(Error::InputConflict);
    }
    let (preservation, after) = derive_anti_bloat_delta(
        &Sha256ScopeDigest,
        &saved.input,
        &saved.review,
        &authored.finding_id,
        authored.disposition,
        &authored.delta,
    )
    .map_err(|_| Error::InputConflict)?;
    if authored.disposition != AntiBloatDisposition::Narrow {
        return Err(Error::InputConflict);
    }
    store
        .apply_preserved_delta(authored, &saved.input, &preservation, &after)
        .await
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedAntiBloatSend {
    pub state: AntiBloatAttemptState,
    pub permit: Option<crate::AntiBloatSendPermit>,
}

pub async fn prepare_anti_bloat_send(
    store: &mut dyn AntiBloatStore,
    provider: &dyn AntiBloatRankingProvider,
    review_id: Uuid,
) -> Result<PreparedAntiBloatSend> {
    let saved = store.review(review_id).await?.ok_or(Error::NotFound)?;
    if saved.review_id != review_id {
        return Err(Error::InputConflict);
    }
    if saved.state != AntiBloatAttemptState::Prepared {
        return Ok(PreparedAntiBloatSend {
            state: saved.state,
            permit: None,
        });
    }
    require_current(store, &saved).await?;
    let eligible = saved
        .review
        .findings
        .iter()
        .filter(|finding| finding.rankable)
        .map(|finding| finding.id.clone())
        .collect::<Vec<_>>();
    if eligible.is_empty() {
        return Err(Error::InputConflict);
    }
    let request_bytes = provider.prepare(&crate::AntiBloatRankingMaterial {
        saved: &saved,
        eligible_ids: &eligible,
    })?;
    if request_bytes.is_empty() || provider.adapter_identity().is_empty() {
        return Err(Error::InputConflict);
    }
    let prepared = AntiBloatPreparedRequest {
        sha256: format!("{:x}", Sha256::digest(&request_bytes)),
        bytes: request_bytes,
        material_sha256: crate::anti_bloat_material_sha256(&saved)?,
        adapter_identity: provider.adapter_identity().into(),
    };
    let now_unix_ms = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::BudgetPolicyInvalid)?
            .as_millis(),
    )
    .map_err(|_| Error::BudgetPolicyInvalid)?;
    let policy = store
        .authorized_budget_policy(saved.workspace_id, now_unix_ms)
        .await?
        .ok_or(Error::BudgetPolicyInvalid)?;
    policy.validate().map_err(|_| Error::BudgetPolicyInvalid)?;
    if !policy.is_effective_at(now_unix_ms) {
        return Err(Error::BudgetPolicyInvalid);
    }
    let Some(permit) = store.begin_send(&saved, &prepared, &policy).await? else {
        return Ok(PreparedAntiBloatSend {
            state: store.review(review_id).await?.ok_or(Error::NotFound)?.state,
            permit: None,
        });
    };
    if permit.review_id != review_id || permit.request != prepared {
        return Err(Error::InputConflict);
    }
    Ok(PreparedAntiBloatSend {
        state: AntiBloatAttemptState::Sending,
        permit: Some(permit),
    })
}

pub async fn seal_anti_bloat_response(
    store: &mut dyn AntiBloatStore,
    permit: &crate::AntiBloatSendPermit,
    raw: &[u8],
) -> Result<()> {
    let response_sha256 = format!("{:x}", Sha256::digest(raw));
    store.seal_response(permit, raw, &response_sha256).await
}

pub async fn finalize_anti_bloat_response(
    store: &mut dyn AntiBloatStore,
    provider: &dyn AntiBloatRankingProvider,
    permit: &crate::AntiBloatSendPermit,
    raw: &[u8],
) -> Result<AntiBloatAttemptState> {
    let saved = store
        .review(permit.review_id)
        .await?
        .ok_or(Error::NotFound)?;
    if saved.state != AntiBloatAttemptState::Sending {
        return Err(Error::InputConflict);
    }
    if permit.request.adapter_identity != provider.adapter_identity()
        || permit.request.material_sha256 != crate::anti_bloat_material_sha256(&saved)?
    {
        return Err(Error::InputConflict);
    }
    let sealed = store.authorized_sealed_response(permit).await?;
    if sealed != raw {
        return Err(Error::InputConflict);
    }
    let eligible = saved
        .review
        .findings
        .iter()
        .filter(|finding| finding.rankable)
        .map(|finding| finding.id.clone())
        .collect::<Vec<_>>();
    let ranked: Vec<String> = match provider.parse_sealed(&sealed) {
        Ok(ids) => ids,
        Err(_) => {
            store.mark_send_unknown(permit.review_id).await?;
            return Err(Error::InputConflict);
        }
    };
    // Provider output can only order the complete frozen eligible set.
    let mut expected = eligible;
    let mut actual = ranked.clone();
    expected.sort();
    actual.sort();
    if expected != actual {
        store.mark_send_unknown(permit.review_id).await?;
        return Err(Error::InputConflict);
    }
    store.seal_ranked(permit.review_id, &ranked).await?;
    Ok(AntiBloatAttemptState::Ranked(ranked))
}

async fn require_current(
    store: &mut dyn AntiBloatStore,
    saved: &StoredAntiBloatReview,
) -> Result<()> {
    let current = store
        .authoritative_input(
            saved.workspace_id,
            saved.review.candidate_set_id,
            saved.review.plan_revision,
        )
        .await?
        .ok_or(Error::InputConflict)?;
    if current != saved.input || review_anti_bloat(&Sha256ScopeDigest, &current)? != saved.review {
        return Err(Error::InputConflict);
    }
    Ok(())
}

async fn rank_after_committed_fence<F: Future<Output = Result<()>>>(
    commit: F,
    provider: &dyn AntiBloatRankingProvider,
    permit: &AntiBloatSendPermit,
) -> Result<Result<AntiBloatProviderObservation>> {
    if permit.request.adapter_identity != provider.adapter_identity()
        || permit.request.sha256 != format!("{:x}", Sha256::digest(&permit.request.bytes))
    {
        return Err(Error::InputConflict);
    }
    commit.await?;
    Ok(provider.rank(permit).await)
}

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

        let (mut consume, _, _) = self
            .anti_bloat_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let exhausted = consume
            .anti_bloat_store()
            .ok_or(Error::StorageUnavailable)?
            .consume_budget(&permit, &observation)
            .await?;
        consume.commit().await?;
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

#[cfg(test)]
mod tests;
