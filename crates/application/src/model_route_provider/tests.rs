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
        let exhausted = observation.input_tokens.is_none_or(|n| n < 0 || n > 100)
            || observation.output_tokens.is_none_or(|n| n < 0 || n > 100)
            || observation
                .elapsed_monotonic_ms
                .is_none_or(|n| n < 0 || n > 10_000);
        self.consumed = Some((observation.clone(), exhausted));
        Ok(exhausted)
    }
    async fn consumption_healthy(&mut self, _: &ModelRouteSendPermit) -> Result<Option<bool>> {
        Ok(self.consumed.as_ref().map(|(_, exhausted)| !exhausted))
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

#[tokio::test]
async fn one_call_after_commit_raw_sealed_before_rank_and_replay_no_send() {
    let saved = prepared(ModelRoutePreparation::Prepared);
    let committed = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = FakeProvider {
        calls: calls.clone(),
        committed: committed.clone(),
        fail: false,
    };
    let mut store = Memory::default();
    let ModelRouteSendStart::Started { attempted, permit } =
        prepare_model_route_send(&mut store, &provider, &saved, invocation())
            .await
            .unwrap()
    else {
        panic!("start")
    };
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let observation = attempt_model_route_observed_after_commit(
        async {
            committed.store(true, Ordering::SeqCst);
            Ok(())
        },
        &provider,
        *attempted.clone(),
        permit.clone(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    seal_model_route_raw_response(&mut store, &permit, &observation.raw)
        .await
        .unwrap();
    assert!(!store.consume_budget(&permit, &observation).await.unwrap());
    let outcome = finalize_model_route_sealed_response(&mut store, &saved, &attempted, &permit)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        ModelRouteRankingWireOutcome::Ranked { .. }
    ));
    let evidence = store.evidence.as_ref().unwrap();
    assert_eq!(
        evidence.verify(&saved).unwrap().unwrap().ranked_route_ids,
        ["route-a"]
    );
    assert_eq!(saved.routes.recommended_route_id, None);
    assert_eq!(saved.routes.observed_actual, None);
    assert_eq!(
        prepare_model_route_send(&mut store, &provider, &saved, invocation())
            .await
            .unwrap(),
        ModelRouteSendStart::Replay
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn disabled_and_unknown_are_no_call_without_provider_attempt() {
    let committed = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = FakeProvider {
        calls: calls.clone(),
        committed,
        fail: false,
    };
    let mut store = Memory::default();
    assert_eq!(
        prepare_model_route_send(
            &mut store,
            &DisabledModelRouteRankingProvider,
            &prepared(ModelRoutePreparation::Prepared),
            invocation(),
        )
        .await
        .unwrap(),
        ModelRouteSendStart::NoCall(ModelRouteRunNoCall::ProviderUnavailable)
    );
    assert_eq!(
        prepare_model_route_send(
            &mut store,
            &provider,
            &prepared(ModelRoutePreparation::UnknownWorkFacts),
            invocation(),
        )
        .await
        .unwrap(),
        ModelRouteSendStart::NoCall(ModelRouteRunNoCall::Preparation(
            ModelRoutePreparation::UnknownWorkFacts
        ))
    );
    assert_eq!(store.no_calls, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn no_trusted_policy_blocks_send_and_unknown_or_overrun_usage_blocks_ranking() {
    let saved = prepared(ModelRoutePreparation::Prepared);
    let provider = FakeProvider {
        calls: Arc::new(AtomicUsize::new(0)),
        committed: Arc::new(AtomicBool::new(true)),
        fail: false,
    };
    let mut no_policy = Memory::default();
    no_policy.policy_enabled = false;
    assert_eq!(
        prepare_model_route_send(&mut no_policy, &provider, &saved, invocation()).await,
        Err(Error::BudgetPolicyInvalid)
    );
    assert!(no_policy.sent.is_none());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);

    for (input_tokens, output_tokens) in [(None, Some(3)), (Some(101), Some(3))] {
        let mut store = Memory::default();
        let ModelRouteSendStart::Started { attempted, permit } =
            prepare_model_route_send(&mut store, &provider, &saved, invocation())
                .await
                .unwrap()
        else {
            panic!("start")
        };
        let raw = provider
            .attempt_prepared(*attempted.clone(), permit.clone())
            .await
            .unwrap();
        seal_model_route_raw_response(&mut store, &permit, &raw)
            .await
            .unwrap();
        let observation = ModelRouteProviderObservation {
            raw: raw.clone(),
            input_tokens,
            output_tokens,
            elapsed_monotonic_ms: Some(1),
        };
        assert!(store.consume_budget(&permit, &observation).await.unwrap());
        assert!(store.consume_budget(&permit, &observation).await.unwrap());
        assert_eq!(
            store.consumption_healthy(&permit).await.unwrap(),
            Some(false)
        );
        assert_eq!(
            finalize_model_route_sealed_response(&mut store, &saved, &attempted, &permit).await,
            Err(Error::InputConflict)
        );
        assert!(store.evidence.is_none());
    }
}

#[tokio::test]
async fn malformed_sealed_raw_and_uncertain_send_never_retry() {
    let saved = prepared(ModelRoutePreparation::Prepared);
    let committed = Arc::new(AtomicBool::new(true));
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = FakeProvider {
        calls: calls.clone(),
        committed,
        fail: true,
    };
    let mut store = Memory::default();
    let ModelRouteSendStart::Started { attempted, permit } =
        prepare_model_route_send(&mut store, &provider, &saved, invocation())
            .await
            .unwrap()
    else {
        panic!("start")
    };
    assert_eq!(
        attempt_model_route_after_commit(
            async { Ok(()) },
            &provider,
            *attempted.clone(),
            permit.clone()
        )
        .await
        .unwrap(),
        Err(Error::TransportUnavailable)
    );
    store.mark_send_unknown(&permit).await.unwrap();
    assert_eq!(
        prepare_model_route_send(&mut store, &provider, &saved, invocation())
            .await
            .unwrap(),
        ModelRouteSendStart::Replay
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let mut second = Memory::default();
    let ModelRouteSendStart::Started { attempted, permit } =
        prepare_model_route_send(&mut second, &provider, &saved, invocation())
            .await
            .unwrap()
    else {
        panic!("start")
    };
    seal_model_route_raw_response(&mut second, &permit, b"{malformed")
        .await
        .unwrap();
    assert_eq!(
        finalize_model_route_sealed_response(&mut second, &saved, &attempted, &permit).await,
        Err(Error::InvalidArguments)
    );
    assert_eq!(second.sealed.as_deref(), Some(b"{malformed".as_slice()));
    assert_eq!(
        prepare_model_route_send(&mut second, &provider, &saved, invocation())
            .await
            .unwrap(),
        ModelRouteSendStart::Replay
    );
}

#[test]
fn model_route_wire_and_digest_golden_vectors() {
    let eligible = prepared(ModelRoutePreparation::Prepared);
    let no_call = ModelRouteAttemptSnapshot {
        attempt_id: Uuid::from_u128(107),
        state: crate::ModelRouteAttemptState::NoCall,
        no_call_reason: Some("provider_unavailable".into()),
        request_sha256: None,
        response_sha256: None,
    };
    let raw_sealed = ModelRouteAttemptSnapshot {
        attempt_id: Uuid::from_u128(108),
        state: crate::ModelRouteAttemptState::RawSealed,
        no_call_reason: None,
        request_sha256: Some("a".repeat(64)),
        response_sha256: Some("b".repeat(64)),
    };
    let abstain = crate::CapturedModelRouteDecision {
        id: Uuid::from_u128(109),
        prepared: eligible.clone(),
        input: crate::ModelRouteDecisionInput::Abstain,
        outcome: crate::ModelRouteDecisionOutcome::Abstained {
            reason: crate::ModelRouteAbstainReason::Explicit,
        },
        routes: eligible.routes.clone(),
    };
    for (name, bytes, golden, digest) in [
        (
            "eligible",
            serde_json::to_vec(&eligible).unwrap(),
            include_str!("tests/eligible_golden.json"),
            "b215dfcc7c0e36f013395367627a2d84299091cae971db1aac427ab72b491a47",
        ),
        (
            "no_call",
            serde_json::to_vec(&no_call).unwrap(),
            include_str!("tests/no_call_golden.json"),
            "19676169e39e30227cde58b96b894b9dec95e85f1cc0cdb918122897a3ff8d35",
        ),
        (
            "raw_sealed",
            serde_json::to_vec(&raw_sealed).unwrap(),
            include_str!("tests/raw_sealed_golden.json"),
            "84cd0d1ace27c19da5a0f923dabd75dfbd3454b24c0c2a32441c3da897655a7a",
        ),
        (
            "abstain",
            serde_json::to_vec(&abstain).unwrap(),
            include_str!("tests/abstain_golden.json"),
            "6afcc2c6cb52c099939edaed2ac696e5aca33175dd2347a47bb80e70fe2e9973",
        ),
    ] {
        assert_eq!(
            std::str::from_utf8(&bytes).unwrap(),
            golden.trim_end(),
            "{name}"
        );
        assert_eq!(model_route_wire_sha256(&bytes), digest, "{name}");
    }
    assert_eq!(
        serde_json::from_str::<PreparedModelRouteRecommendation>(include_str!(
            "tests/eligible_golden.json"
        ))
        .unwrap(),
        eligible
    );
    assert_eq!(
        serde_json::from_str::<ModelRouteAttemptSnapshot>(include_str!(
            "tests/no_call_golden.json"
        ))
        .unwrap(),
        no_call
    );
    assert_eq!(
        serde_json::from_str::<ModelRouteAttemptSnapshot>(include_str!(
            "tests/raw_sealed_golden.json"
        ))
        .unwrap(),
        raw_sealed
    );
    assert_eq!(
        serde_json::from_str::<crate::CapturedModelRouteDecision>(include_str!(
            "tests/abstain_golden.json"
        ))
        .unwrap(),
        abstain
    );
}
