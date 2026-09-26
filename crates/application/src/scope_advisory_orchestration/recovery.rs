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
        let Some(saved) = read
            .advisory_dispatch_receipt(workspace, opportunity.id)
            .await?
            .filter(|saved| saved.observation.is_some())
        else {
            return Ok(None);
        };
        if saved.opportunity.authorized_actor_id != actor {
            return Err(Error::Forbidden);
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
    if stored.authored_request_digest.as_deref() != Some(&digest)
        || opportunity.workflow_occurrence_key != request.request_id.to_string()
        || opportunity.target_kind != "scope_candidate_set"
        || opportunity.target_id != Some(request.candidate_set_id)
        || record.candidate_set_id != request.candidate_set_id
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
