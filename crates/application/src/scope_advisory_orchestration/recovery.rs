use super::*;
use crate::{
    AdvisoryProviderReceiptUsage, StoredAdvisoryProviderReceipt, StoredScopeManifestRecord,
};

impl WorkspaceService {
    pub(super) async fn scope_receipt_for_replay(
        &self,
        read: &mut dyn crate::UnitOfWork,
        workspace: Uuid,
        actor: Uuid,
        session: Uuid,
        request: &RunScopeAdvisory,
        existing: Option<&AdvisoryOpportunity>,
    ) -> Result<Option<(StoredAdvisoryProviderReceipt, StoredScopeManifestRecord)>> {
        let Some(opportunity) = existing.filter(|opportunity| {
            matches!(
                opportunity.state,
                AdvisoryOpportunityState::AwaitingResponse
                    | AdvisoryOpportunityState::Unresolved
                    | AdvisoryOpportunityState::Advised
                    | AdvisoryOpportunityState::Failed
                    | AdvisoryOpportunityState::Invalidated
            )
        }) else {
            return Ok(None);
        };
        if opportunity.authorized_actor_id != actor {
            return Err(Error::Forbidden);
        }
        if opportunity.session_id != session
            || opportunity.request_preference != request.request_preference
            || opportunity.session_preference != request.session_preference
        {
            return Err(Error::InputConflict);
        }
        let Some(saved) = read
            .advisory_dispatch_receipt(workspace, opportunity.id)
            .await?
            .filter(|saved| saved.observation.is_some())
        else {
            return Ok(None);
        };
        if saved.opportunity != *opportunity {
            return Err(Error::InputConflict);
        }
        let stored = read
            .scope_advisory_manifest_by_request_key(workspace, &request.request_id.to_string())
            .await?
            .ok_or(Error::InputConflict)?;
        Ok(Some((saved, stored)))
    }
    pub(super) async fn recover_scope_receipt(
        &self,
        context: &RequestContext,
        request: &RunScopeAdvisory,
        tenant: Uuid,
        saved: StoredAdvisoryProviderReceipt,
        stored: StoredScopeManifestRecord,
    ) -> Result<ScopeAdvisoryOutcome> {
        validate_receipt_replay_binding(request, &saved, &stored)?;
        if matches!(
            saved.opportunity.state,
            AdvisoryOpportunityState::Advised
                | AdvisoryOpportunityState::Failed
                | AdvisoryOpportunityState::Invalidated
        ) {
            return self
                .replay_authored_scope_advisory(
                    context,
                    saved.opportunity.workspace_id,
                    saved.opportunity,
                    &stored.record,
                )
                .await;
        }
        let continuation = crate::AdvisoryDispatchContinuation::from_saved(
            tenant,
            saved.opportunity.workspace_id,
            &saved.opportunity,
            &saved.dispatch,
        )?;
        let usage = scope_recovery_usage(self.scope_advice_provider.as_ref(), &saved);
        let (saved, consumption) = self
            .consume_committed_advisory_observation(&continuation, usage)
            .await?;
        // Exact frozen body/full manifest; no new preparation, send permit, or legacy cache.
        let prepared = prepared_from_receipt(&saved, &stored.record.manifest)?;
        self.finish_scope_receipt(
            context,
            request,
            &saved,
            consumption,
            prepared,
            &stored.record.manifest,
            None,
        )
        .await
    }
}

pub(super) fn scope_recovery_usage(
    provider: &dyn crate::ScopeAdviceProvider,
    saved: &StoredAdvisoryProviderReceipt,
) -> AdvisoryProviderReceiptUsage {
    if saved.dispatch.state == AdvisoryDispatchState::Sealed {
        AdvisoryProviderReceiptUsage {
            input_tokens: saved
                .dispatch
                .input_tokens
                .and_then(|n| u64::try_from(n).ok()),
            output_tokens: saved
                .dispatch
                .output_tokens
                .and_then(|n| u64::try_from(n).ok()),
        }
    } else {
        provider
            .usage_from_sealed_response(saved)
            .unwrap_or_default()
    }
}

fn validate_receipt_replay_binding(
    request: &RunScopeAdvisory,
    saved: &StoredAdvisoryProviderReceipt,
    stored: &StoredScopeManifestRecord,
) -> Result<()> {
    let opportunity = &saved.opportunity;
    let record = &stored.record;
    let digest = authored_request_digest(
        request
            .authored_scope_set
            .as_ref()
            .ok_or(Error::InputConflict)?,
    )?;
    if opportunity.capability != tect_domain::AdvisoryCapability::ScopeDecomposition
        || opportunity.decision_point
            != tect_domain::AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection
        || stored.authored_request_digest.as_deref() != Some(&digest)
        || opportunity.workflow_occurrence_key != request.request_id.to_string()
        || opportunity.session_preference != request.session_preference
        || opportunity.request_preference != request.request_preference
        || opportunity.target_kind != "scope_candidate_set"
        || opportunity.target_id != Some(request.candidate_set_id)
        || record.candidate_set_id != request.candidate_set_id
        || record.manifest.source.candidate_set_id != request.candidate_set_id
        || record.opportunity_id != opportunity.id
        || record.config_revision != opportunity.config_revision
        || record.opportunity_material_digest != opportunity.material_digest
        || record.manifest.whole_set_digest != opportunity.material_digest
        || opportunity.work_revision != Some(record.manifest.source.candidate_set_revision)
        || request
            .authored_scope_set
            .as_ref()
            .map(|set| set.expected_candidate_set_revision)
            != Some(record.manifest.source.candidate_set_revision)
    {
        return Err(Error::InputConflict);
    }
    Ok(())
}

fn prepared_from_receipt(
    saved: &StoredAdvisoryProviderReceipt,
    manifest: &tect_domain::ScopeConstructorManifest,
) -> Result<PreparedScopeAdviceAttempt> {
    let snapshot = &saved.configuration_snapshot;
    let field = |key| {
        snapshot
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or(Error::InputConflict)
    };
    let profile = snapshot
        .get("provider_profile_ref")
        .and_then(|value| value.get("id"))
        .and_then(serde_json::Value::as_str)
        .ok_or(Error::InputConflict)?;
    if snapshot["request_body_length"] != saved.request_payload.len()
        || snapshot["request_body_sha256"] != saved.request_payload_sha256
        || saved.request_payload_sha256 != sha256(&saved.request_payload)
        || saved.dispatch.payload_digest != saved.request_payload_sha256
    {
        return Err(Error::InputConflict);
    }
    let request = ScopeAdviceRequest::from_manifest(&Sha256ScopeDigest, manifest)?;
    PreparedScopeAdviceAttempt::new(
        request,
        saved.request_payload.clone(),
        profile.to_owned(),
        saved.dispatch.model.clone(),
        field("destination")?,
        field("wire_version")?,
    )
    .map_err(|_| Error::InputConflict)
}

impl WorkspaceService {
    pub(super) async fn finalize_prepared_scope_stale(
        &self,
        context: &RequestContext,
        workspace_id: Uuid,
        opportunity_id: Uuid,
        candidate_set_id: Uuid,
        expected_source_digest: &str,
        reason: AdvisoryReason,
    ) -> Result<ScopeAdvisoryOutcome> {
        let disposition = crate::ScopePreparedAdvisoryDisposition {
            opportunity_id,
            candidate_set_id,
            expected_source_digest: expected_source_digest.to_owned(),
            reason,
        };
        let (mut tx, workspace, _) = self
            .scope_transaction(context, TransactionMode::ReadWrite)
            .await?;
        if workspace.id != workspace_id {
            return Err(Error::InputConflict);
        }
        tx.finalize_prepared_scope_advisory_without_dispatch(workspace_id, &disposition)
            .await?;
        let opportunity = tx
            .advisory_opportunity_for_dispatch(workspace_id, opportunity_id)
            .await?;
        let opportunity = validate_terminalized_pre_dispatch_opportunity(opportunity)?;
        tx.commit().await?;
        Ok(ScopeAdvisoryOutcome {
            opportunity,
            advice: None,
        })
    }

    pub(super) async fn replay_authored_scope_advisory(
        &self,
        context: &RequestContext,
        workspace_id: Uuid,
        opportunity: AdvisoryOpportunity,
        record: &ScopeManifestRecord,
    ) -> Result<ScopeAdvisoryOutcome> {
        let advice = if opportunity.state == AdvisoryOpportunityState::Advised {
            let (mut replay, workspace, _) = self
                .scope_transaction(context, TransactionMode::ReadOnly)
                .await?;
            if workspace.id != workspace_id {
                return Err(Error::InputConflict);
            }
            let advice = replay
                .guarded_scope_advice(workspace_id, opportunity.id)
                .await?
                .ok_or(Error::StorageUnavailable)?;
            tect_domain::validate_guarded_advice_binding(
                &Sha256ScopeDigest,
                &record.manifest,
                &advice,
            )?;
            replay.commit().await?;
            Some(advice)
        } else {
            None
        };
        Ok(ScopeAdvisoryOutcome {
            opportunity,
            advice,
        })
    }

    pub(super) async fn scope_transaction(
        &self,
        context: &RequestContext,
        mode: TransactionMode,
    ) -> Result<(
        Box<dyn crate::UnitOfWork>,
        tect_domain::Workspace,
        tect_domain::Session,
    )> {
        let (mut tx, identity) = self.authorized(context, mode).await?;
        if mode == TransactionMode::ReadWrite {
            tx.lock_native_session(identity.host_id, &context.native_session_id)
                .await?;
        }
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        Ok((tx, workspace, session))
    }
}
