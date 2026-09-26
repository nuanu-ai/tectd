//! One guarded Slice 03 attempt. The preparation receipt is a lookup key,
//! never authority to call a provider or to record a verification result.

use crate::{
    PipelineDispatchCapability, PipelineStartedDispatchPermit, PreparedPipelineRecommendation,
    PreparedPipelineRecommendationAttempt, SealedPipelineRecommendationResponse, TransactionMode,
    WorkspaceService,
};
use sha2::{Digest, Sha256};
use tect_domain::{
    AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryDispatchAuthorization,
    AdvisoryDispatchOutcome, AdvisoryOpportunityState, AdvisoryReason, AdvisoryRetryBasis,
    AdvisorySendCertainty, Error, PipelineRecommendationRanking, RequestContext, Result,
    WorkspaceAdvisoryMode,
};
use uuid::Uuid;

#[cfg(test)]
mod receipt_tests;
mod receipts;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunPipelineRecommendation {
    pub opportunity_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineRecommendationRun {
    NoCall {
        opportunity_id: Uuid,
        reason: AdvisoryReason,
    },
    Stale {
        opportunity_id: Uuid,
    },
    SendUnknown {
        opportunity_id: Uuid,
        dispatch_id: Uuid,
    },
    BudgetExhausted {
        opportunity_id: Uuid,
        dispatch_id: Uuid,
    },
    Ranked {
        opportunity_id: Uuid,
        dispatch_id: Uuid,
        ranked_ids: Vec<String>,
    },
    Abstained {
        opportunity_id: Uuid,
        dispatch_id: Uuid,
    },
}

fn validate_saved(
    saved: &PreparedPipelineRecommendation,
    workspace_id: Uuid,
    opportunity_id: Uuid,
) -> Result<()> {
    let opportunity = &saved.opportunity;
    saved.manifest.validate_digest()?;
    if opportunity.id != opportunity_id
        || opportunity.workspace_id != workspace_id
        || opportunity.capability != AdvisoryCapability::PipelineRecommendation
        || opportunity.decision_point
            != AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen
        || opportunity.material_digest != saved.manifest.digest
        || opportunity.target_id != Some(saved.context.work_node_id)
        || opportunity.work_revision != Some(saved.context.work_node_revision)
        || saved.context.verification_contract_digest != saved.manifest.digest
        || saved.context.compatibility_policy_digest != saved.manifest.compatibility_policy_digest
        || saved.context.eligible_option_ids
            != saved
                .manifest
                .options
                .iter()
                .map(|option| option.id.clone())
                .collect::<Vec<_>>()
    {
        return Err(Error::InputConflict);
    }
    Ok(())
}

fn authorization(
    saved: &PreparedPipelineRecommendation,
    attempt: &PreparedPipelineRecommendationAttempt,
) -> Result<AdvisoryDispatchAuthorization> {
    let identity = attempt.identity();
    let configuration_snapshot = serde_json::json!({
        "provider_profile_ref": identity.provider,
        "model_configuration": { "model": identity.model },
        "destination": identity.destination,
        "wire_version": identity.wire_version,
        "request_body_sha256": attempt.body_sha256(),
    });
    let configuration_digest = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&configuration_snapshot).map_err(Error::invalid_arguments_from)?
        )
    );
    let authorization = AdvisoryDispatchAuthorization {
        dispatch_id: Uuid::new_v4(),
        opportunity_id: saved.opportunity.id,
        predecessor_dispatch_id: None,
        attempt_number: 1,
        retry_basis: AdvisoryRetryBasis::Initial,
        provider: identity.provider.clone(),
        model: identity.model.clone(),
        configuration_snapshot,
        configuration_digest,
        material_digest: saved.manifest.digest.clone(),
        payload_digest: attempt.body_sha256().to_owned(),
        request_payload: attempt.body().to_vec(),
    };
    authorization.validate()?;
    Ok(authorization)
}

impl WorkspaceService {
    pub async fn run_pipeline_recommendation(
        &self,
        context: &RequestContext,
        request: &RunPipelineRecommendation,
    ) -> Result<PipelineRecommendationRun> {
        if request.opportunity_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        // Every invocation reloads the exact durable capture. A replay never
        // receives another send permit, even if the first send is uncertain.
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let saved = tx
            .pipeline_recommendation_store()
            .ok_or(Error::Forbidden)?
            .pipeline_recommendation_by_opportunity(workspace.id, request.opportunity_id)
            .await?
            .ok_or(Error::NotFound)?;
        validate_saved(&saved, workspace.id, request.opportunity_id)?;
        if saved.opportunity.authorized_actor_id != identity.principal_id {
            return Err(Error::Forbidden);
        }
        // Actor-bound recovery precedes all fresh source/configuration gates.
        if let Some(receipt) = tx
            .advisory_dispatch_receipt(workspace.id, saved.opportunity.id)
            .await?
        {
            if receipt.opportunity.authorized_actor_id != identity.principal_id {
                return Err(Error::Forbidden);
            }
            let tenant = identity.tenant_id;
            tx.commit().await?;
            return self
                .recover_pipeline_receipt(context, tenant, saved, receipt)
                .await;
        }
        if saved.opportunity.state == AdvisoryOpportunityState::NoCall {
            tx.commit().await?;
            return Ok(PipelineRecommendationRun::NoCall {
                opportunity_id: saved.opportunity.id,
                reason: saved.opportunity.primary_reason,
            });
        }
        if self
            .validate_pipeline_recommendation_definitions(&saved.manifest)
            .is_err()
        {
            tx.commit().await?;
            return Ok(PipelineRecommendationRun::Stale {
                opportunity_id: saved.opportunity.id,
            });
        }
        if !self.pipeline_policy_matches(&saved.context.compatibility_policy_digest)? {
            tx.commit().await?;
            return Ok(PipelineRecommendationRun::Stale {
                opportunity_id: saved.opportunity.id,
            });
        }
        if saved.opportunity.state != AdvisoryOpportunityState::Prepared
            || saved.opportunity.primary_reason != AdvisoryReason::RecommendationPrepared
        {
            return Err(Error::InputConflict);
        }
        let config = tx.advisory_config(workspace.id).await?;
        if config.revision != saved.opportunity.config_revision
            || config.mode != WorkspaceAdvisoryMode::Optional
            || !config.provider_configured()
            || !saved.manifest.should_call()
            || !self.pipeline_policy_matches(&saved.context.compatibility_policy_digest)?
            || !tx
                .pipeline_recommendation_store()
                .ok_or(Error::Forbidden)?
                .pipeline_recommendation_is_current(workspace.id, &saved)
                .await?
        {
            tx.commit().await?;
            return Ok(PipelineRecommendationRun::Stale {
                opportunity_id: saved.opportunity.id,
            });
        }
        let attempt = self.pipeline_recommendation_provider.prepare(&saved)?;
        if attempt.opportunity_id() != saved.opportunity.id
            || attempt.manifest_digest() != saved.manifest.digest
            || config
                .provider_profile_ref
                .as_ref()
                .map(|profile| profile.id.as_str())
                != Some(attempt.identity().provider.as_str())
            || config
                .model_configuration
                .as_ref()
                .map(|model| model.model.as_str())
                != Some(attempt.identity().model.as_str())
        {
            return Err(Error::InputConflict);
        }
        let grant = authorization(&saved, &attempt)?;
        let dispatch_store = tx
            .pipeline_recommendation_dispatch_store()
            .ok_or(Error::Forbidden)?;
        let authorized = dispatch_store
            .authorize_pipeline_dispatch(
                &PipelineDispatchCapability::internal(),
                workspace.id,
                config.revision,
                &grant,
            )
            .await?;
        if authorized.id != grant.dispatch_id {
            return Err(Error::InputConflict);
        }
        let started = dispatch_store
            .start_pipeline_dispatch(
                &PipelineDispatchCapability::internal(),
                workspace.id,
                authorized.id,
            )
            .await?;
        let tenant = identity.tenant_id;
        // COMMIT is the critical boundary. A failed commit cannot mint a permit.
        tx.commit().await?;
        let permit =
            PipelineStartedDispatchPermit::after_committed_start(&started, &grant, &attempt)?;
        let continuation = crate::AdvisoryDispatchContinuation::after_committed_start(
            tenant,
            workspace.id,
            &saved.opportunity,
            &started,
            &grant,
        )?;
        let saved_attempt = PreparedPipelineRecommendationAttempt::new(
            &saved,
            attempt.identity().clone(),
            attempt.body().to_vec(),
        )?;
        let monotonic_start = std::time::Instant::now();
        let observed = self
            .pipeline_recommendation_provider
            .observe_prepared(attempt, permit)
            .await;
        let elapsed_ms = i64::try_from(monotonic_start.elapsed().as_millis()).unwrap_or(i64::MAX);
        let raw = observed.unwrap_or(crate::AdvisoryProviderReceiptObservation {
            response_payload: None,
            http_status: None,
            input_tokens: None,
            output_tokens: None,
            response_complete: false,
            original_transport_context: Some(crate::AdvisoryProviderTransportContext {
                send_certainty: AdvisorySendCertainty::SentUnknown,
                outcome: AdvisoryDispatchOutcome::ProviderFailure,
                raw_response_ref: None,
                provider_failure_code: None,
            }),
        });
        // Immutable transport facts commit before any post-send caller fence.
        let receipt = self
            .seal_committed_advisory_observation(&continuation, &raw, elapsed_ms)
            .await?;
        let usage = self
            .pipeline_recommendation_provider
            .usage_from_sealed_response(&receipt)
            .unwrap_or_default();
        let (receipt, consumption) = self
            .consume_committed_advisory_observation(&continuation, usage)
            .await?;
        self.finish_pipeline_receipt(context, saved, receipt, consumption, saved_attempt)
            .await
    }
}
