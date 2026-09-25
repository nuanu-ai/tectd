//! Slice 04 application policy. All source/plan facts come from the store port.
use crate::{
    AntiBloatAttemptState, AntiBloatAuthoredDelta, AntiBloatNoCall, AntiBloatRankingProvider,
    AntiBloatStore, Sha256ScopeDigest, StoredAntiBloatReview,
};
use tect_domain::{
    AdvisoryRequestPreference, AntiBloatDisposition, CandidateDeltaReceipt, Error, Result,
    WorkspaceAdvisoryMode, check_anti_bloat_delta, review_anti_bloat,
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
        if workspace_id.is_nil()
            || actor_id.is_nil()
            || candidate_set_id.is_nil()
            || expected_revision < 1
        {
            return Err(Error::InvalidArguments);
        }
        let mode = self.store.advisory_mode(workspace_id).await?;
        let input = self
            .store
            .authoritative_input(workspace_id, candidate_set_id, expected_revision)
            .await?
            .ok_or(Error::NotFound)?;
        if input.manifest.source.candidate_set_id != candidate_set_id
            || input.manifest.source.candidate_set_revision != expected_revision
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
        self.store
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

    /// `begin_send` is a committed, one-use fence. Transport failure leaves a
    /// send-unknown state; this method never retries it.
    pub async fn rank_once(&mut self, review_id: Uuid) -> Result<AntiBloatAttemptState> {
        let saved = self.store.review(review_id).await?.ok_or(Error::NotFound)?;
        if saved.review_id != review_id {
            return Err(Error::InputConflict);
        }
        if saved.state != AntiBloatAttemptState::Prepared {
            return Ok(saved.state);
        }
        self.require_current(&saved).await?;
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
        if !self.store.begin_send(&saved).await? {
            return Ok(self
                .store
                .review(review_id)
                .await?
                .ok_or(Error::NotFound)?
                .state);
        }
        let ranked = match self.provider.rank(&saved.review, &eligible).await {
            Ok(ranked) => ranked,
            Err(_) => {
                self.store.mark_send_unknown(review_id).await?;
                return Ok(AntiBloatAttemptState::SendUnknown);
            }
        };
        // Provider output can only order the complete frozen eligible set.
        let mut expected = eligible;
        let mut actual = ranked.clone();
        expected.sort();
        actual.sort();
        if expected != actual {
            self.store.mark_send_unknown(review_id).await?;
            return Err(Error::InputConflict);
        }
        self.store.seal_ranked(review_id, &ranked).await?;
        Ok(AntiBloatAttemptState::Ranked(ranked))
    }

    /// Applies only an explicit agent-authored delta that the pure checker has
    /// proven against the full frozen plan and a fresh authoritative snapshot.
    pub async fn disposition_and_apply(
        &mut self,
        authored: &AntiBloatAuthoredDelta,
    ) -> Result<CandidateDeltaReceipt> {
        let saved = self
            .store
            .review(authored.review_id)
            .await?
            .ok_or(Error::NotFound)?;
        if saved.review_id != authored.review_id {
            return Err(Error::InputConflict);
        }
        self.require_current(&saved).await?;
        let preservation = check_anti_bloat_delta(
            &Sha256ScopeDigest,
            &saved.input,
            &saved.review,
            &authored.finding_id,
            authored.disposition,
            &authored.delta,
            &authored.after,
        )
        .map_err(|_| Error::InputConflict)?;
        if authored.disposition != AntiBloatDisposition::Narrow {
            return Err(Error::InputConflict);
        }
        self.store
            .apply_preserved_delta(
                authored.review_id,
                &saved.input,
                &authored.finding_id,
                authored.disposition,
                &preservation,
                &authored.delta,
            )
            .await
    }

    async fn require_current(&mut self, saved: &StoredAntiBloatReview) -> Result<()> {
        let current = self
            .store
            .authoritative_input(
                saved.workspace_id,
                saved.review.candidate_set_id,
                saved.review.plan_revision,
            )
            .await?
            .ok_or(Error::InputConflict)?;
        if current != saved.input
            || review_anti_bloat(&Sha256ScopeDigest, &current)? != saved.review
        {
            return Err(Error::InputConflict);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
