//! Public recommendation-only workflow. No recommended model is executed.
use tect_domain::{Error, RequestContext, Result};
use uuid::Uuid;

use crate::{
    CapturedModelRouteDisposition, DecideModelRouteRecommendation,
    DispositionModelRouteRecommendation, ModelRouteDecisionInput, ModelRouteDispositionAction,
    ModelRouteInvocation, ModelRouteSendStart, PrepareModelRouteRecommendation,
    PreparedModelRouteRecommendation, TransactionMode, WorkspaceService,
    attempt_model_route_observed_after_commit, finalize_model_route_sealed_response,
    prepare_model_route_send,
};

pub use tect_domain::ModelRouteView;

impl WorkspaceService {
    pub async fn prepare_model_route(
        &self,
        context: &RequestContext,
        request: &PrepareModelRouteRecommendation,
    ) -> Result<PreparedModelRouteRecommendation> {
        let (mut writer, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        writer
            .lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let session = writer
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *writer, context, &identity, &session).await?;
        let (mut reader, _) = self.authorized(context, TransactionMode::ReadWrite).await?;
        let mut effective = request.clone();
        effective.workspace_id = workspace.id;
        let (host_capabilities, catalogue_provider) = self.model_route_advisory_inputs();
        let prepared = effective
            .prepare(
                writer
                    .model_route_recommendation_store()
                    .ok_or(Error::Forbidden)?,
                reader
                    .model_route_selection_read()
                    .ok_or(Error::Forbidden)?,
                host_capabilities,
                catalogue_provider,
            )
            .await?;
        writer
            .model_route_recommendation_store()
            .ok_or(Error::Forbidden)?
            .validate_current(&prepared)
            .await?;
        writer.commit().await?;
        Ok(prepared)
    }

    pub async fn get_model_route(
        &self,
        context: &RequestContext,
        preparation_request_key: &str,
    ) -> Result<ModelRouteView> {
        if preparation_request_key.is_empty() || preparation_request_key.len() > 256 {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let prepared = tx
            .model_route_recommendation_store()
            .ok_or(Error::Forbidden)?
            .by_request(workspace.id, preparation_request_key)
            .await?
            .ok_or(Error::NotFound)?;
        tx.model_route_recommendation_store()
            .ok_or(Error::Forbidden)?
            .validate_current(&prepared)
            .await?;
        let attempt = tx
            .model_route_attempt_store()
            .ok_or(Error::Forbidden)?
            .by_preparation(
                workspace.id,
                preparation_request_key,
                ModelRouteInvocation {
                    session_id: session.id,
                },
            )
            .await?;
        let decision = tx
            .model_route_decision_store()
            .ok_or(Error::Forbidden)?
            .decision_by_preparation(workspace.id, preparation_request_key)
            .await?;
        let disposition = match &decision {
            Some(value) => {
                tx.model_route_decision_store()
                    .ok_or(Error::Forbidden)?
                    .disposition_by_decision(workspace.id, value.id)
                    .await?
            }
            None => None,
        };
        tx.commit().await?;
        Ok(ModelRouteView {
            preparation: prepared,
            attempt,
            decision,
            disposition,
        })
    }

    pub async fn run_model_route(
        &self,
        context: &RequestContext,
        preparation_request_key: &str,
    ) -> Result<ModelRouteView> {
        if preparation_request_key.is_empty() || preparation_request_key.len() > 256 {
            return Err(Error::InvalidArguments);
        }
        let (mut start, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        start
            .lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let session = start
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *start, context, &identity, &session).await?;
        let prepared = start
            .model_route_recommendation_store()
            .ok_or(Error::Forbidden)?
            .by_request(workspace.id, preparation_request_key)
            .await?
            .ok_or(Error::NotFound)?;
        start
            .model_route_recommendation_store()
            .ok_or(Error::Forbidden)?
            .validate_current(&prepared)
            .await?;
        let started = prepare_model_route_send(
            start.model_route_attempt_store().ok_or(Error::Forbidden)?,
            &*self.model_route_ranking_provider,
            &prepared,
            ModelRouteInvocation {
                session_id: session.id,
            },
        )
        .await?;
        match started {
            ModelRouteSendStart::NoCall(_) => {
                start.commit().await?;
                self.finish_model_route_decision(
                    context,
                    preparation_request_key,
                    ModelRouteDecisionInput::NoCall,
                )
                .await?;
            }
            ModelRouteSendStart::Replay => {
                start.commit().await?;
                let view = self
                    .get_model_route(context, preparation_request_key)
                    .await?;
                if view.decision.is_none() {
                    match view.attempt.as_ref().map(|a| a.state) {
                        Some(crate::ModelRouteAttemptState::NoCall) => {
                            self.finish_model_route_decision(
                                context,
                                preparation_request_key,
                                ModelRouteDecisionInput::NoCall,
                            )
                            .await?
                        }
                        Some(crate::ModelRouteAttemptState::Parsed) => {
                            self.finish_from_sealed(context, preparation_request_key)
                                .await?
                        }
                        Some(crate::ModelRouteAttemptState::RawSealed) => {
                            let (mut recovery, _) =
                                self.authorized(context, TransactionMode::ReadWrite).await?;
                            let recovered = recovery
                                .model_route_attempt_store()
                                .ok_or(Error::Forbidden)?
                                .recover_raw_sealed(
                                    workspace.id,
                                    preparation_request_key,
                                    ModelRouteInvocation {
                                        session_id: session.id,
                                    },
                                )
                                .await?;
                            if let Some((attempted, permit)) = recovered {
                                let store = recovery
                                    .model_route_attempt_store()
                                    .ok_or(Error::Forbidden)?;
                                let healthy = match store.consumption_healthy(&permit).await? {
                                    Some(value) => value,
                                    None => {
                                        let raw = store
                                            .sealed_response(&permit)
                                            .await?
                                            .ok_or(Error::StaleContext)?;
                                        store
                                            .consume_budget(
                                                &permit,
                                                &crate::ModelRouteProviderObservation {
                                                    raw,
                                                    input_tokens: None,
                                                    output_tokens: None,
                                                    elapsed_monotonic_ms: None,
                                                },
                                            )
                                            .await?;
                                        false
                                    }
                                };
                                if healthy
                                    && finalize_model_route_sealed_response(
                                        store, &prepared, &attempted, &permit,
                                    )
                                    .await
                                    .is_ok()
                                {
                                    recovery.commit().await?;
                                    self.finish_from_sealed(context, preparation_request_key)
                                        .await?;
                                } else {
                                    recovery.commit().await?;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            ModelRouteSendStart::Started { attempted, permit } => {
                let observation = match attempt_model_route_observed_after_commit(
                    start.commit(),
                    &*self.model_route_ranking_provider,
                    *attempted.clone(),
                    permit.clone(),
                )
                .await?
                {
                    Ok(observation) => observation,
                    Err(_) => {
                        self.record_committed_model_route_failure(identity.tenant_id, &permit)
                            .await?;
                        return self.get_model_route(context, preparation_request_key).await;
                    }
                };
                // The provider may return after the invoking session is
                // revoked. Preserve its raw response under the committed,
                // exact one-use permit before attempting any new user auth.
                self.seal_committed_model_route_response(
                    identity.tenant_id,
                    &permit,
                    &observation.raw,
                )
                .await?;
                let exhausted = self
                    .consume_committed_model_route_budget(identity.tenant_id, &permit, &observation)
                    .await?;
                if exhausted {
                    return self.get_model_route(context, preparation_request_key).await;
                }
                let (mut parse, parse_identity) =
                    self.authorized(context, TransactionMode::ReadWrite).await?;
                parse
                    .lock_native_session(parse_identity.host_id, &context.native_session_id)
                    .await?;
                let parse_session = parse
                    .session(parse_identity.host_id, &context.native_session_id)
                    .await?
                    .ok_or(Error::WorkspaceNotOpen)?;
                let parse_workspace =
                    Self::validate_binding(&mut *parse, context, &parse_identity, &parse_session)
                        .await?;
                if parse_workspace.id != permit.workspace_id {
                    return Err(Error::StaleContext);
                }
                let parsed = finalize_model_route_sealed_response(
                    parse.model_route_attempt_store().ok_or(Error::Forbidden)?,
                    &prepared,
                    &attempted,
                    &permit,
                )
                .await;
                match parsed {
                    Ok(_) => {
                        parse.commit().await?;
                        self.finish_from_sealed(context, preparation_request_key)
                            .await?;
                    }
                    Err(_) => return self.get_model_route(context, preparation_request_key).await,
                }
            }
        }
        self.get_model_route(context, preparation_request_key).await
    }

    async fn finish_from_sealed(&self, context: &RequestContext, key: &str) -> Result<()> {
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let saved = tx
            .model_route_decision_store()
            .ok_or(Error::Forbidden)?
            .sealed_provider_ranking(workspace.id, key)
            .await?
            .ok_or(Error::StaleContext)?;
        let prepared = tx
            .model_route_recommendation_store()
            .ok_or(Error::Forbidden)?
            .by_request(workspace.id, key)
            .await?
            .ok_or(Error::NotFound)?;
        let input = match saved.verify(&prepared)? {
            Some(ranking) => ModelRouteDecisionInput::Ranking(ranking),
            None => ModelRouteDecisionInput::Abstain,
        };
        tx.commit().await?;
        self.finish_model_route_decision(context, key, input).await
    }

    async fn finish_model_route_decision(
        &self,
        context: &RequestContext,
        key: &str,
        input: ModelRouteDecisionInput,
    ) -> Result<()> {
        let (mut writer, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        writer
            .lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let session = writer
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *writer, context, &identity, &session).await?;
        if writer
            .model_route_decision_store()
            .ok_or(Error::Forbidden)?
            .decision_by_preparation(workspace.id, key)
            .await?
            .is_some()
        {
            writer.commit().await?;
            return Ok(());
        }
        let (mut reader, _) = self.authorized(context, TransactionMode::ReadWrite).await?;
        DecideModelRouteRecommendation {
            id: Uuid::new_v4(),
            workspace_id: workspace.id,
            preparation_request_key: key.into(),
            input,
        }
        .decide(
            reader
                .model_route_recommendation_store()
                .ok_or(Error::Forbidden)?,
            writer
                .model_route_decision_store()
                .ok_or(Error::Forbidden)?,
        )
        .await?;
        writer.commit().await
    }

    pub async fn disposition_model_route(
        &self,
        context: &RequestContext,
        decision_id: Uuid,
        disposition_id: Uuid,
        action: ModelRouteDispositionAction,
        rationale: String,
    ) -> Result<CapturedModelRouteDisposition> {
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let decision = tx
            .model_route_decision_store()
            .ok_or(Error::Forbidden)?
            .decision_by_id(workspace.id, decision_id)
            .await?
            .ok_or(Error::NotFound)?;
        tx.model_route_recommendation_store()
            .ok_or(Error::Forbidden)?
            .validate_current(&decision.prepared)
            .await?;
        let saved = DispositionModelRouteRecommendation {
            id: disposition_id,
            decision_id,
            workspace_id: workspace.id,
            actor_id: identity.principal_id,
            action,
            rationale,
        }
        .record(tx.model_route_decision_store().ok_or(Error::Forbidden)?)
        .await?;
        tx.commit().await?;
        Ok(saved)
    }
}
