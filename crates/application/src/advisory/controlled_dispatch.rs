use super::*;

impl WorkspaceService {
    /// Controlled Slice-00 fixture boundary. No public production route reaches
    /// this while the capability remains unavailable.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) async fn controlled_advisory_dispatch(
        &self,
        context: &tect_domain::RequestContext,
        request: &ControlledAdvisoryDispatch,
    ) -> Result<ControlledAdvisoryResult> {
        if request.dispatch_id.is_nil()
            || request.opportunity_id.is_nil()
            || request.payload.is_empty()
            || request.attempt_number < 1
        {
            return Err(Error::InvalidArguments);
        }
        let (provider, adapter_version) = self
            .advisory_provider
            .identity()
            .ok_or(Error::TransportUnavailable)?;
        if !matches!(
            request.retry_basis,
            AdvisoryRetryBasis::Initial | AdvisoryRetryBasis::ProvenNotSent
        ) {
            return Err(Error::InvalidArguments);
        }
        let lifecycle = AdvisoryLifecycleCapability::internal();

        let (mut authorize_tx, workspace, _) = self
            .advisory_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let opportunity = authorize_tx
            .advisory_opportunity_for_dispatch(workspace.id, request.opportunity_id)
            .await?;
        let config = authorize_tx.advisory_config(workspace.id).await?;
        let profile = config
            .provider_profile_ref
            .clone()
            .ok_or(Error::InvalidConfiguration)?;
        let model_configuration = config
            .model_configuration
            .clone()
            .ok_or(Error::InvalidConfiguration)?;
        let configuration_snapshot = serde_json::json!({
            "provider_profile_ref": profile,
            "model_configuration": model_configuration,
            "adapter_version": adapter_version,
        });
        let configuration_bytes =
            serde_json::to_vec(&configuration_snapshot).map_err(Error::invalid_arguments_from)?;
        let authorization = AdvisoryDispatchAuthorization {
            dispatch_id: request.dispatch_id,
            opportunity_id: request.opportunity_id,
            predecessor_dispatch_id: request.predecessor_dispatch_id,
            attempt_number: request.attempt_number,
            retry_basis: request.retry_basis,
            provider: provider.into(),
            model: model_configuration.model.clone(),
            configuration_snapshot,
            configuration_digest: sha256(&configuration_bytes),
            material_digest: opportunity.material_digest.clone(),
            payload_digest: sha256(&request.payload),
            request_payload: request.payload.clone(),
        };
        authorization.validate()?;
        let authorized = authorize_tx
            .authorize_advisory_dispatch(&lifecycle, workspace.id, config.revision, &authorization)
            .await?;
        authorize_tx.commit().await?;

        let (mut start_tx, workspace, _) = self
            .advisory_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let started = start_tx
            .start_advisory_dispatch(&lifecycle, workspace.id, authorized.id)
            .await?;
        start_tx.commit().await?;
        if !started.should_send {
            if started.dispatch.state == AdvisoryDispatchState::Sealed {
                let (mut finalize_tx, workspace, _) = self
                    .advisory_transaction(context, TransactionMode::ReadWrite)
                    .await?;
                let finalized = finalize_tx
                    .finalize_advisory_opportunity(
                        &lifecycle,
                        workspace.id,
                        opportunity.id,
                        opportunity.config_revision,
                        &started.dispatch,
                    )
                    .await?;
                finalize_tx.commit().await?;
                return Ok(ControlledAdvisoryResult {
                    opportunity_id: opportunity.id,
                    advice_eligible: finalized.state == AdvisoryOpportunityState::Advised,
                    dispatch: started.dispatch,
                });
            }
            return Ok(ControlledAdvisoryResult {
                opportunity_id: opportunity.id,
                advice_eligible: false,
                dispatch: started.dispatch,
            });
        }

        let provider_request = AdvisoryProviderRequest {
            dispatch_id: authorized.id,
            provider_profile_ref: profile,
            model_configuration,
            payload: request.payload.clone(),
        };
        let started_at = std::time::Instant::now();
        let observation =
            attempt_provider_once(&*self.advisory_provider, &provider_request).await?;
        let elapsed = i64::try_from(started_at.elapsed().as_millis()).unwrap_or(i64::MAX);
        let seal = AdvisoryDispatchSeal {
            dispatch_id: authorized.id,
            send_certainty: observation.send_certainty,
            outcome: observation.outcome,
            response_payload: observation.response_payload,
            input_tokens: observation.input_tokens,
            output_tokens: observation.output_tokens,
            latency_ms: Some(elapsed),
            raw_response_ref: observation.raw_response_ref,
        };
        seal.validate()?;
        let (mut seal_tx, workspace, _) = self
            .advisory_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let sealed = seal_tx
            .seal_advisory_dispatch(&lifecycle, workspace.id, &seal)
            .await?;
        seal_tx.commit().await?;

        let (mut finalize_tx, workspace, _) = self
            .advisory_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let finalized = finalize_tx
            .finalize_advisory_opportunity(
                &lifecycle,
                workspace.id,
                opportunity.id,
                opportunity.config_revision,
                &sealed,
            )
            .await?;
        finalize_tx.commit().await?;
        Ok(ControlledAdvisoryResult {
            opportunity_id: opportunity.id,
            advice_eligible: finalized.state == AdvisoryOpportunityState::Advised,
            dispatch: sealed,
        })
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) async fn cancel_controlled_advisory_dispatch(
        &self,
        context: &tect_domain::RequestContext,
        dispatch_id: uuid::Uuid,
    ) -> Result<tect_domain::AdvisoryDispatchCancellation> {
        if dispatch_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let lifecycle = AdvisoryLifecycleCapability::internal();
        let (mut tx, workspace, _) = self
            .advisory_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let result = tx
            .cancel_advisory_dispatch(&lifecycle, workspace.id, dispatch_id)
            .await?;
        tx.commit().await?;
        Ok(result)
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) async fn reconcile_controlled_advisory_dispatch(
        &self,
        context: &tect_domain::RequestContext,
        evidence: &AdvisoryReconciliationEvidence,
    ) -> Result<ControlledAdvisoryResult> {
        evidence.validate()?;
        let lifecycle = AdvisoryLifecycleCapability::internal();
        let (mut tx, workspace, _) = self
            .advisory_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let dispatch = tx
            .reconcile_advisory_dispatch(&lifecycle, workspace.id, evidence)
            .await?;
        let opportunity = tx
            .advisory_opportunity_for_dispatch(workspace.id, dispatch.opportunity_id)
            .await?;
        let finalized = tx
            .finalize_advisory_opportunity(
                &lifecycle,
                workspace.id,
                opportunity.id,
                opportunity.config_revision,
                &dispatch,
            )
            .await?;
        tx.commit().await?;
        Ok(ControlledAdvisoryResult {
            opportunity_id: opportunity.id,
            advice_eligible: finalized.state == AdvisoryOpportunityState::Advised,
            dispatch,
        })
    }
}
