use super::*;
use crate::{DisabledModelRouteRankingProvider, ModelRouteAttemptSnapshot};
use async_trait::async_trait;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tect_domain::{
    AdvisoryBudgetCeilings, AdvisoryBudgetPolicy, AdvisoryRequestPreference,
    MODEL_ROUTE_CATALOGUE_SCHEMA, MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA,
    MODEL_ROUTE_RANKING_WIRE_SCHEMA, MatrixPlanningSelection, ModelRoute, ModelRouteCatalogue,
    ModelRouteFact, ModelRouteFactProvenance, ModelRouteHostCapabilities,
    ModelRouteRankingWireRequest, ModelRouteRecord, ModelRouteSelectionLink, ModelRouteWorkContext,
};
use uuid::Uuid;

mod native;
mod scenarios;

fn invocation() -> ModelRouteInvocation {
    ModelRouteInvocation {
        session_id: Uuid::new_v4(),
    }
}

fn test_policy() -> AdvisoryBudgetPolicy {
    let id = Uuid::from_u128(201);
    let ceilings = AdvisoryBudgetCeilings {
        provider_calls: 2,
        input_tokens: 100,
        output_tokens: 100,
        request_utf8_bytes: 1_000_000,
        elapsed_monotonic_ms: 10_000,
        retry_dispatches: 1,
    };
    AdvisoryBudgetPolicy::new(
        id,
        1,
        AdvisoryBudgetPolicy::digest_for(id, 1, 0, i64::MAX, ceilings),
        0,
        i64::MAX,
        ceilings,
        Uuid::from_u128(202),
        "a".repeat(128),
    )
    .unwrap()
}

fn caller<T>(value: T, node: Uuid) -> ModelRouteFact<T> {
    ModelRouteFact::Known {
        value,
        provenance: ModelRouteFactProvenance::Caller {
            source_ref: "receipt#/facts".into(),
            work_node_id: node,
            work_node_revision: 1,
        },
    }
}

fn prepared(state: ModelRoutePreparation) -> PreparedModelRouteRecommendation {
    let node = Uuid::from_u128(101);
    let mut work = ModelRouteWorkContext {
        approved_matrix_selection: MatrixPlanningSelection {
            task_id: Uuid::from_u128(102),
            task_revision: 1,
            disposition_id: Uuid::from_u128(103),
            selected_choice_id: "choice-a".into(),
            expected_input_digest: "a".repeat(64),
            expected_choice_set_digest: "b".repeat(64),
            expected_verification_digest: "c".repeat(64),
            mapped_draft_node_indices: vec![0],
        },
        selection_link: ModelRouteSelectionLink {
            candidate_set_id: Uuid::from_u128(104),
            caller_request_id: Uuid::from_u128(105),
            mapped_draft_node_index: 0,
            mapped_work_node_id: node,
            mapped_work_node_revision: 1,
        },
        role: caller("agent".into(), node),
        tool: caller("code".into(), node),
        data_class: caller("internal".into(), node),
        host_capabilities: ModelRouteHostCapabilities {
            schema: MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
            version: 1,
            capabilities: vec!["model-api".into()],
        }
        .fact()
        .unwrap(),
        remaining_budget_units: caller(20, node),
        available_latency_ms: caller(100, node),
    };
    if state == ModelRoutePreparation::UnknownWorkFacts {
        work.role = ModelRouteFact::Unknown;
    }
    let catalogue = ModelRouteCatalogue {
        schema: MODEL_ROUTE_CATALOGUE_SCHEMA.into(),
        version: 1,
        routes: vec![ModelRoute {
            id: "route-a".into(),
            provider: "configured".into(),
            model: "candidate-model".into(),
            effort: "medium".into(),
            enabled: true,
            allowed_matrix_choice_ids: vec!["choice-a".into()],
            allowed_roles: vec!["agent".into()],
            allowed_tools: vec!["code".into()],
            allowed_data_classes: vec!["internal".into()],
            required_host_capabilities: vec!["model-api".into()],
            minimum_budget_units: 10,
            minimum_latency_ms: 50,
        }],
    };
    let eligible = catalogue.eligible(&work).unwrap();
    PreparedModelRouteRecommendation {
        workspace_id: Uuid::from_u128(106),
        request_key: "prepare-1".into(),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        advisory_config_revision: 1,
        work,
        catalogue: Some(catalogue),
        eligible: Some(eligible),
        preparation: state,
        routes: ModelRouteRecord {
            requested_route_id: None,
            recommended_route_id: None,
            observed_actual: None,
        },
    }
}

struct FakeProvider {
    calls: Arc<AtomicUsize>,
    committed: Arc<AtomicBool>,
    fail: bool,
}

#[async_trait]
impl ModelRouteRankingProvider for FakeProvider {
    fn prepare(
        &self,
        saved: &PreparedModelRouteRecommendation,
    ) -> Result<ModelRoutePreparedAttempt> {
        ModelRoutePreparedAttempt::new(ModelRouteRankingWireRequest::new(
            saved.workspace_id,
            &saved.request_key,
            &saved.work,
            saved.catalogue.as_ref().unwrap(),
            saved.eligible.as_ref().unwrap(),
            "jev-adviser",
        )?)
    }
    async fn attempt_prepared(
        &self,
        attempted: ModelRoutePreparedAttempt,
        _: ModelRouteSendPermit,
    ) -> Result<Vec<u8>> {
        assert!(self.committed.load(Ordering::SeqCst));
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            return Err(Error::TransportUnavailable);
        }
        Ok(serde_json::json!({
            "schema": MODEL_ROUTE_RANKING_WIRE_SCHEMA,
            "binding_digest": attempted.request.binding_digest,
            "adviser_model": "jev-adviser",
            "outcome": {"kind":"ranked","route_ids":["route-a"]},
        })
        .to_string()
        .into_bytes())
    }
    async fn attempt_prepared_observed(
        &self,
        attempted: ModelRoutePreparedAttempt,
        permit: ModelRouteSendPermit,
    ) -> Result<ModelRouteProviderObservation> {
        let raw = self.attempt_prepared(attempted, permit).await?;
        Ok(ModelRouteProviderObservation {
            raw,
            http_status: None,
            input_tokens: Some(4),
            output_tokens: Some(3),
            elapsed_monotonic_ms: Some(1),
        })
    }
}

struct Memory {
    sent: Option<ModelRouteSendPermit>,
    sealed: Option<Vec<u8>>,
    evidence: Option<ModelRouteSealedRankingEvidence>,
    unknown: bool,
    no_calls: usize,
    policy_enabled: bool,
    consumed: Option<(ModelRouteProviderObservation, bool)>,
    observation: Option<ModelRouteProviderObservation>,
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            sent: None,
            sealed: None,
            evidence: None,
            unknown: false,
            no_calls: 0,
            policy_enabled: true,
            consumed: None,
            observation: None,
        }
    }
}

#[async_trait]
impl ModelRouteAttemptStore for Memory {
    async fn authorized_budget_policy(
        &mut self,
        _: Uuid,
        _: i64,
    ) -> Result<Option<AdvisoryBudgetPolicy>> {
        Ok(self.policy_enabled.then(test_policy))
    }
    async fn by_preparation(
        &mut self,
        _: Uuid,
        _: &str,
        _: ModelRouteInvocation,
    ) -> Result<Option<ModelRouteAttemptSnapshot>> {
        Ok(None)
    }
    async fn record_no_call(
        &mut self,
        _: &PreparedModelRouteRecommendation,
        _: ModelRouteInvocation,
        _: ModelRouteRunNoCall,
    ) -> Result<()> {
        self.no_calls += 1;
        Ok(())
    }
    async fn begin_send(
        &mut self,
        saved: &PreparedModelRouteRecommendation,
        _: ModelRouteInvocation,
        attempted: &ModelRoutePreparedAttempt,
        policy: &AdvisoryBudgetPolicy,
        _required_profile: Option<&str>,
    ) -> Result<Option<ModelRouteSendPermit>> {
        attempted.verify(saved)?;
        if self.sent.is_some() {
            return Ok(None);
        }
        let permit = ModelRouteSendPermit {
            attempt_id: Uuid::new_v4(),
            workspace_id: saved.workspace_id,
            preparation_request_key: saved.request_key.clone(),
            request_sha256: attempted.request_sha256.clone(),
            policy_id: policy.id(),
            policy_version: policy.version(),
            policy_digest: policy.digest().to_owned(),
        };
        self.sent = Some(permit.clone());
        Ok(Some(permit))
    }
    async fn seal_raw_response(
        &mut self,
        permit: &ModelRouteSendPermit,
        raw: &[u8],
        digest: &str,
    ) -> Result<()> {
        if self.sent.as_ref() != Some(permit)
            || self.sealed.is_some()
            || model_route_wire_sha256(raw) != digest
        {
            return Err(Error::InputConflict);
        }
        self.sealed = Some(raw.into());
        Ok(())
    }
    async fn sealed_response(&mut self, permit: &ModelRouteSendPermit) -> Result<Option<Vec<u8>>> {
        if self.sent.as_ref() != Some(permit) {
            return Err(Error::InputConflict);
        }
        Ok(self.sealed.clone())
    }
    async fn consume_budget(
        &mut self,
        permit: &ModelRouteSendPermit,
        observation: &ModelRouteProviderObservation,
    ) -> Result<bool> {
        if self.sent.as_ref() != Some(permit) || self.sealed.as_ref() != Some(&observation.raw) {
            return Err(Error::InputConflict);
        }
        if let Some((existing, exhausted)) = &self.consumed {
            return if existing == observation {
                Ok(*exhausted)
            } else {
                Err(Error::InputConflict)
            };
        }
        let exhausted = observation
            .input_tokens
            .is_none_or(|n| !(0..=100).contains(&n))
            || observation
                .output_tokens
                .is_none_or(|n| !(0..=100).contains(&n))
            || observation
                .elapsed_monotonic_ms
                .is_none_or(|n| !(0..=10_000).contains(&n));
        self.consumed = Some((observation.clone(), exhausted));
        Ok(exhausted)
    }
    async fn consumption_healthy(&mut self, _: &ModelRouteSendPermit) -> Result<Option<bool>> {
        Ok(self.consumed.as_ref().map(|(_, exhausted)| !exhausted))
    }
    async fn seal_observation(
        &mut self,
        permit: &ModelRouteSendPermit,
        observation: &ModelRouteProviderObservation,
    ) -> Result<()> {
        self.seal_raw_response(
            permit,
            &observation.raw,
            &model_route_wire_sha256(&observation.raw),
        )
        .await?;
        self.observation = Some(observation.clone());
        Ok(())
    }
    async fn sealed_observation(
        &mut self,
        permit: &ModelRouteSendPermit,
    ) -> Result<Option<ModelRouteProviderObservation>> {
        if let Some(observation) = &self.observation {
            return Ok(Some(observation.clone()));
        }
        Ok(self
            .sealed_response(permit)
            .await?
            .map(|raw| ModelRouteProviderObservation {
                raw,
                http_status: None,
                input_tokens: None,
                output_tokens: None,
                elapsed_monotonic_ms: None,
            }))
    }
    async fn capture_provider_outcome(
        &mut self,
        evidence: &ModelRouteSealedRankingEvidence,
        provider: &dyn ModelRouteRankingProvider,
    ) -> Result<()> {
        let observation = self
            .sealed_observation(&evidence.permit)
            .await?
            .ok_or(Error::StaleContext)?;
        if provider.parse_sealed(&evidence.attempted, &observation)? != evidence.outcome {
            return Err(Error::InputConflict);
        }
        self.capture_sealed_outcome(evidence).await
    }
    async fn capture_sealed_outcome(
        &mut self,
        evidence: &ModelRouteSealedRankingEvidence,
    ) -> Result<()> {
        if self.sent.as_ref() != Some(&evidence.permit)
            || self.sealed.as_ref() != Some(&evidence.raw_response)
            || self
                .consumed
                .as_ref()
                .is_none_or(|(_, exhausted)| *exhausted)
        {
            return Err(Error::InputConflict);
        }
        if self
            .evidence
            .as_ref()
            .is_some_and(|saved| saved != evidence)
        {
            return Err(Error::InputConflict);
        }
        self.evidence = Some(evidence.clone());
        Ok(())
    }
    async fn mark_send_unknown(&mut self, permit: &ModelRouteSendPermit) -> Result<()> {
        if self.sent.as_ref() != Some(permit) {
            return Err(Error::InputConflict);
        }
        self.unknown = true;
        Ok(())
    }
}
