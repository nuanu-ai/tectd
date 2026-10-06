use super::*;
use crate::{
    AdvisoryProviderReceiptUsage, PIPELINE_ADVICE_INTERPRETATION_VERSION,
    PipelineAdviceInterpretation, StoredAdvisoryProviderReceipt,
};
use tect_domain::AdvisoryDispatchState;

impl WorkspaceService {
    pub(super) async fn recover_pipeline_receipt(
        &self,
        context: &RequestContext,
        tenant: Uuid,
        saved: PreparedPipelineRecommendation,
        receipt: StoredAdvisoryProviderReceipt,
    ) -> Result<PipelineRecommendationRun> {
        validate_receipt(&saved, &receipt)?;
        // Terminal old receipts are replayed before the common consumer: a
        // historical seal cannot acquire a second accounting event.
        if matches!(
            receipt.opportunity.state,
            AdvisoryOpportunityState::Advised
                | AdvisoryOpportunityState::Failed
                | AdvisoryOpportunityState::Invalidated
        ) {
            return self
                .replay_pipeline_receipt(context, &saved, &receipt)
                .await;
        }
        if receipt.observation.is_none() {
            return Ok(PipelineRecommendationRun::SendUnknown {
                opportunity_id: saved.opportunity.id,
                dispatch_id: receipt.dispatch.id,
            });
        }
        let continuation = crate::AdvisoryDispatchContinuation::from_saved(
            tenant,
            saved.opportunity.workspace_id,
            &receipt.opportunity,
            &receipt.dispatch,
        )?;
        let usage = if receipt.dispatch.state == AdvisoryDispatchState::Sealed {
            AdvisoryProviderReceiptUsage {
                input_tokens: receipt
                    .dispatch
                    .input_tokens
                    .and_then(|n| u64::try_from(n).ok()),
                output_tokens: receipt
                    .dispatch
                    .output_tokens
                    .and_then(|n| u64::try_from(n).ok()),
            }
        } else {
            self.pipeline_recommendation_provider
                .usage_from_sealed_response(&receipt)
                .unwrap_or_default()
        };
        let prepared = frozen_attempt(&saved, &receipt)?;
        let (receipt, consumption) = self
            .consume_committed_advisory_observation(&continuation, usage)
            .await?;
        self.finish_pipeline_receipt(context, saved, receipt, consumption, prepared)
            .await
    }

    async fn replay_pipeline_receipt(
        &self,
        context: &RequestContext,
        saved: &PreparedPipelineRecommendation,
        receipt: &StoredAdvisoryProviderReceipt,
    ) -> Result<PipelineRecommendationRun> {
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        if workspace.id != saved.opportunity.workspace_id
            || identity.principal_id != receipt.opportunity.authorized_actor_id
        {
            return Err(Error::Forbidden);
        }
        if receipt.opportunity.state != AdvisoryOpportunityState::Advised {
            tx.commit().await?;
            return Ok(terminal_result(&receipt.opportunity, receipt.dispatch.id));
        }
        let normalized = tx
            .pipeline_recommendation_store()
            .ok_or(Error::Forbidden)?
            .pipeline_advice_interpretation(workspace.id, saved.opportunity.id)
            .await?;
        let ranking = match normalized {
            Some(value) => {
                validate_interpretation(saved, receipt, &value)?;
                value.ranking
            }
            None => {
                let attempt = frozen_attempt(saved, receipt)?;
                let sealed = if receipt.observation.is_some() {
                    sealed_response(receipt, &attempt)?
                } else {
                    let legacy = tx
                        .pipeline_recommendation_dispatch_store()
                        .ok_or(Error::Forbidden)?
                        .pipeline_dispatch_for_replay(workspace.id, saved.opportunity.id)
                        .await?
                        .ok_or(Error::InputConflict)?;
                    if legacy.dispatch != receipt.dispatch {
                        return Err(Error::InputConflict);
                    }
                    SealedPipelineRecommendationResponse::from_saved(
                        &legacy.dispatch,
                        &attempt,
                        &legacy.request_payload,
                        legacy.response_payload,
                        &legacy.response_sha256,
                    )?
                };
                let value = self
                    .pipeline_recommendation_provider
                    .parse_sealed_response(&saved.manifest, &attempt, &sealed)?;
                value.validate(&saved.manifest)?;
                value
            }
        };
        tx.commit().await?;
        Ok(ranking_result(
            saved.opportunity.id,
            receipt.dispatch.id,
            ranking,
        ))
    }

    pub(super) async fn finish_pipeline_receipt(
        &self,
        context: &RequestContext,
        saved: PreparedPipelineRecommendation,
        receipt: StoredAdvisoryProviderReceipt,
        consumption: tect_domain::AdvisoryBudgetConsumption,
        attempt: PreparedPipelineRecommendationAttempt,
    ) -> Result<PipelineRecommendationRun> {
        validate_receipt(&saved, &receipt)?;
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        if workspace.id != saved.opportunity.workspace_id
            || identity.principal_id != receipt.opportunity.authorized_actor_id
        {
            return Err(Error::Forbidden);
        }
        let id = saved.opportunity.id;
        let dispatch_id = receipt.dispatch.id;
        let raw_healthy = receipt.observation.as_ref().is_some_and(|raw| {
            raw.response_complete
                && raw
                    .response_payload
                    .as_ref()
                    .is_some_and(|bytes| !bytes.is_empty())
                && raw
                    .http_status
                    .is_none_or(|status| (200..300).contains(&status))
                && raw
                    .original_transport_context
                    .as_ref()
                    .is_some_and(|transport| {
                        transport.send_certainty == AdvisorySendCertainty::Sent
                            && transport.outcome == AdvisoryDispatchOutcome::ProviderResponse
                    })
        });
        if consumption.unknown_usage || consumption.exhausted_after_response || !raw_healthy {
            let terminal = tx
                .finalize_pipeline_advisory_without_advice(
                    workspace.id,
                    id,
                    saved.opportunity.config_revision,
                    &receipt.dispatch,
                )
                .await?;
            tx.commit().await?;
            return Ok(terminal_result(&terminal, dispatch_id));
        }
        let config = tx.advisory_config(workspace.id).await?;
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| Error::BudgetPolicyInvalid)?
                .as_millis(),
        )
        .map_err(|_| Error::BudgetPolicyInvalid)?;
        let budget_current = match tx.advisory_budget_policy_store() {
            Some(store) => store
                .authorized_budget_policy(workspace.id, now)
                .await?
                .is_some_and(|policy| policy.validate().is_ok()),
            None => false,
        };
        let current = config.revision == saved.opportunity.config_revision
            && budget_current
            && config.mode == WorkspaceAdvisoryMode::Optional
            && config.provider_configured()
            && config.provider_profile_ref.as_ref().map(|p| p.id.as_str())
                == Some(receipt.dispatch.provider.as_str())
            && config
                .model_configuration
                .as_ref()
                .map(|m| m.model.as_str())
                == Some(receipt.dispatch.model.as_str())
            && self.pipeline_policy_matches(&saved.context.compatibility_policy_digest)?
            && self
                .validate_pipeline_recommendation_definitions(&saved.manifest)
                .is_ok()
            && self
                .pipeline_recommendation_provider
                .prepare(&saved)
                .is_ok_and(|fresh| {
                    fresh.identity() == attempt.identity() && fresh.body() == attempt.body()
                })
            && tx
                .pipeline_recommendation_store()
                .ok_or(Error::Forbidden)?
                .pipeline_recommendation_is_current(workspace.id, &saved)
                .await?;
        if !current {
            let terminal = tx
                .finalize_pipeline_advisory_without_advice(
                    workspace.id,
                    id,
                    saved.opportunity.config_revision,
                    &receipt.dispatch,
                )
                .await?;
            tx.commit().await?;
            return Ok(terminal_result(&terminal, dispatch_id));
        }
        // Parsing is pure and follows every current authorization/source gate.
        let parsed = sealed_response(&receipt, &attempt).and_then(|sealed| {
            let ranking = self
                .pipeline_recommendation_provider
                .parse_sealed_response(&saved.manifest, &attempt, &sealed)?;
            ranking.validate(&saved.manifest)?;
            Ok((ranking, sealed.sha256().to_owned()))
        });
        let Ok((ranking, response_sha256)) = parsed else {
            let terminal = tx
                .finalize_pipeline_advisory_without_advice(
                    workspace.id,
                    id,
                    saved.opportunity.config_revision,
                    &receipt.dispatch,
                )
                .await?;
            tx.commit().await?;
            return Ok(terminal_result(&terminal, dispatch_id));
        };
        let interpretation = PipelineAdviceInterpretation {
            opportunity_id: id,
            dispatch_id,
            manifest_digest: saved.manifest.digest.clone(),
            response_sha256,
            contract_version: PIPELINE_ADVICE_INTERPRETATION_VERSION,
            ranking: ranking.clone(),
        };
        let inserted = tx
            .pipeline_recommendation_store()
            .ok_or(Error::Forbidden)?
            .insert_pipeline_advice_interpretation(workspace.id, &interpretation)
            .await?;
        if inserted != interpretation {
            return Err(Error::InputConflict);
        }
        tx.finalize_advisory_opportunity(
            &crate::AdvisoryLifecycleCapability::internal(),
            workspace.id,
            id,
            config.revision,
            &receipt.dispatch,
        )
        .await?;
        tx.commit().await?;
        Ok(ranking_result(id, dispatch_id, ranking))
    }
}

fn validate_receipt(
    saved: &PreparedPipelineRecommendation,
    receipt: &StoredAdvisoryProviderReceipt,
) -> Result<()> {
    if receipt.opportunity.id != saved.opportunity.id
        || receipt.opportunity.workspace_id != saved.opportunity.workspace_id
        || receipt.opportunity.authorized_actor_id != saved.opportunity.authorized_actor_id
        || receipt.opportunity.capability != saved.opportunity.capability
        || receipt.opportunity.decision_point != saved.opportunity.decision_point
        || receipt.opportunity.target_kind != saved.opportunity.target_kind
        || receipt.opportunity.target_id != saved.opportunity.target_id
        || receipt.opportunity.work_revision != saved.opportunity.work_revision
        || receipt.opportunity.config_revision != saved.opportunity.config_revision
        || receipt.opportunity.material_digest != saved.manifest.digest
        || receipt.dispatch.opportunity_id != saved.opportunity.id
        || receipt.dispatch.material_digest != saved.manifest.digest
    {
        return Err(Error::InputConflict);
    }
    Ok(())
}

fn frozen_attempt(
    saved: &PreparedPipelineRecommendation,
    receipt: &StoredAdvisoryProviderReceipt,
) -> Result<PreparedPipelineRecommendationAttempt> {
    let field = |key| {
        receipt
            .configuration_snapshot
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or(Error::InputConflict)
    };
    let attempt = PreparedPipelineRecommendationAttempt::new(
        saved,
        crate::PipelineProviderIdentity {
            provider: receipt.dispatch.provider.clone(),
            model: receipt.dispatch.model.clone(),
            destination: field("destination")?,
            wire_version: field("wire_version")?,
        },
        receipt.request_payload.clone(),
    )?;
    let configuration_sha = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&receipt.configuration_snapshot)
                .map_err(Error::invalid_arguments_from)?
        )
    );
    if attempt.body_sha256() != receipt.request_payload_sha256
        || attempt.body_sha256() != receipt.dispatch.payload_digest
        || configuration_sha != receipt.dispatch.configuration_digest
        || receipt.configuration_snapshot["provider_profile_ref"].as_str()
            != Some(receipt.dispatch.provider.as_str())
        || receipt.configuration_snapshot["model_configuration"]["model"].as_str()
            != Some(receipt.dispatch.model.as_str())
        || receipt.configuration_snapshot["request_body_sha256"].as_str()
            != Some(attempt.body_sha256())
    {
        return Err(Error::InputConflict);
    }
    Ok(attempt)
}

fn sealed_response(
    receipt: &StoredAdvisoryProviderReceipt,
    attempt: &PreparedPipelineRecommendationAttempt,
) -> Result<SealedPipelineRecommendationResponse> {
    let raw = receipt.observation.as_ref().ok_or(Error::InputConflict)?;
    if !raw.response_complete {
        return Err(Error::InputConflict);
    }
    let bytes = raw.response_payload.clone().ok_or(Error::InputConflict)?;
    let sha = format!("{:x}", Sha256::digest(&bytes));
    SealedPipelineRecommendationResponse::from_saved(
        &receipt.dispatch,
        attempt,
        &receipt.request_payload,
        bytes,
        &sha,
    )
}

fn validate_interpretation(
    saved: &PreparedPipelineRecommendation,
    receipt: &StoredAdvisoryProviderReceipt,
    value: &PipelineAdviceInterpretation,
) -> Result<()> {
    let raw = receipt
        .observation
        .as_ref()
        .and_then(|raw| raw.response_payload.as_ref())
        .ok_or(Error::InputConflict)?;
    if value.opportunity_id != saved.opportunity.id
        || value.dispatch_id != receipt.dispatch.id
        || value.manifest_digest != saved.manifest.digest
        || value.response_sha256 != format!("{:x}", Sha256::digest(raw))
        || value.contract_version != PIPELINE_ADVICE_INTERPRETATION_VERSION
    {
        return Err(Error::InputConflict);
    }
    value.ranking.validate(&saved.manifest)
}

fn ranking_result(
    opportunity_id: Uuid,
    dispatch_id: Uuid,
    ranking: PipelineRecommendationRanking,
) -> PipelineRecommendationRun {
    match ranking {
        PipelineRecommendationRanking::Ranked { ranked_ids } => PipelineRecommendationRun::Ranked {
            opportunity_id,
            dispatch_id,
            ranked_ids,
        },
        PipelineRecommendationRanking::Abstained => PipelineRecommendationRun::Abstained {
            opportunity_id,
            dispatch_id,
        },
    }
}

fn terminal_result(
    opportunity: &tect_domain::AdvisoryOpportunity,
    dispatch_id: Uuid,
) -> PipelineRecommendationRun {
    match opportunity.primary_reason {
        AdvisoryReason::SendUnknown => PipelineRecommendationRun::SendUnknown {
            opportunity_id: opportunity.id,
            dispatch_id,
        },
        AdvisoryReason::BudgetExhaustedAfterResponse => {
            PipelineRecommendationRun::BudgetExhausted {
                opportunity_id: opportunity.id,
                dispatch_id,
            }
        }
        _ => PipelineRecommendationRun::Stale {
            opportunity_id: opportunity.id,
        },
    }
}
