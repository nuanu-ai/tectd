use super::*;
use crate::{ModelRouteRecommendationBasis, PreparedModelRouteRecommendation};
use async_trait::async_trait;
use std::collections::BTreeMap;
use tect_domain::{
    AdvisoryRequestPreference, MODEL_ROUTE_CATALOGUE_SCHEMA, MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA,
    MODEL_ROUTE_RANKING_WIRE_SCHEMA, MatrixPlanningSelection, ModelRoute, ModelRouteCatalogue,
    ModelRouteFact, ModelRouteFactProvenance, ModelRouteHostCapabilities, ModelRouteRanking,
    ModelRouteRankingWireOutcome, ModelRouteRankingWireRequest, ModelRouteRecord,
    ModelRouteSelectionLink, ModelRouteWorkContext,
};

#[derive(Default)]
struct PreparationMemory {
    prepared: Option<PreparedModelRouteRecommendation>,
}

#[derive(Default)]
struct Memory {
    evidence: Option<crate::ModelRouteSealedRankingEvidence>,
    decisions: BTreeMap<Uuid, CapturedModelRouteDecision>,
    dispositions: BTreeMap<Uuid, CapturedModelRouteDisposition>,
    decision_writes: usize,
    disposition_writes: usize,
}

#[async_trait]
impl ModelRouteRecommendationStore for PreparationMemory {
    async fn by_request(
        &mut self,
        workspace_id: Uuid,
        request_key: &str,
    ) -> Result<Option<PreparedModelRouteRecommendation>> {
        Ok(self
            .prepared
            .as_ref()
            .filter(|v| v.workspace_id == workspace_id && v.request_key == request_key)
            .cloned())
    }
    async fn load_basis(
        &mut self,
        _: Uuid,
        _: Uuid,
    ) -> Result<Option<ModelRouteRecommendationBasis>> {
        Ok(None)
    }
    async fn capture(
        &mut self,
        _: &PreparedModelRouteRecommendation,
    ) -> Result<PreparedModelRouteRecommendation> {
        Err(Error::InternalInvariant)
    }
}

#[async_trait]
impl ModelRouteDecisionStore for Memory {
    async fn sealed_provider_ranking(
        &mut self,
        workspace_id: Uuid,
        preparation_request_key: &str,
    ) -> Result<Option<crate::ModelRouteSealedRankingEvidence>> {
        Ok(self
            .evidence
            .as_ref()
            .filter(|proof| {
                proof.permit.workspace_id == workspace_id
                    && proof.permit.preparation_request_key == preparation_request_key
            })
            .cloned())
    }
    async fn decision_by_id(
        &mut self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> Result<Option<CapturedModelRouteDecision>> {
        Ok(self
            .decisions
            .get(&id)
            .filter(|v| v.prepared.workspace_id == workspace_id)
            .cloned())
    }
    async fn decision_by_preparation(
        &mut self,
        workspace_id: Uuid,
        request_key: &str,
    ) -> Result<Option<CapturedModelRouteDecision>> {
        Ok(self
            .decisions
            .values()
            .find(|v| {
                v.prepared.workspace_id == workspace_id && v.prepared.request_key == request_key
            })
            .cloned())
    }
    async fn capture_decision(
        &mut self,
        value: &CapturedModelRouteDecision,
    ) -> Result<CapturedModelRouteDecision> {
        if self.decisions.contains_key(&value.id) {
            return Err(Error::InputConflict);
        }
        self.decision_writes += 1;
        self.decisions.insert(value.id, value.clone());
        Ok(value.clone())
    }
    async fn disposition_by_id(
        &mut self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> Result<Option<CapturedModelRouteDisposition>> {
        Ok(self
            .dispositions
            .get(&id)
            .filter(|v| v.workspace_id == workspace_id)
            .cloned())
    }
    async fn disposition_by_decision(
        &mut self,
        workspace_id: Uuid,
        decision_id: Uuid,
    ) -> Result<Option<CapturedModelRouteDisposition>> {
        Ok(self
            .dispositions
            .values()
            .find(|v| v.workspace_id == workspace_id && v.decision_id == decision_id)
            .cloned())
    }
    async fn capture_disposition(
        &mut self,
        value: &CapturedModelRouteDisposition,
    ) -> Result<CapturedModelRouteDisposition> {
        if self.dispositions.contains_key(&value.id) {
            return Err(Error::InputConflict);
        }
        self.disposition_writes += 1;
        self.dispositions.insert(value.id, value.clone());
        Ok(value.clone())
    }
}

fn caller<T>(value: T, node_id: Uuid) -> ModelRouteFact<T> {
    ModelRouteFact::Known {
        value,
        provenance: ModelRouteFactProvenance::Caller {
            source_ref: "native_planning_receipts:exact#/draft/nodes/0/model_route_facts".into(),
            work_node_id: node_id,
            work_node_revision: 1,
        },
    }
}

fn prepared(preparation: ModelRoutePreparation) -> PreparedModelRouteRecommendation {
    let workspace_id = Uuid::new_v4();
    let candidate_set_id = Uuid::new_v4();
    let caller_request_id = Uuid::new_v4();
    let node_id = Uuid::new_v4();
    let selection = MatrixPlanningSelection {
        task_id: Uuid::new_v4(),
        task_revision: 1,
        disposition_id: Uuid::new_v4(),
        selected_choice_id: "choice-a".into(),
        expected_input_digest: "a".repeat(64),
        expected_choice_set_digest: "b".repeat(64),
        expected_verification_digest: "c".repeat(64),
        mapped_draft_node_indices: vec![0],
    };
    let mut work = ModelRouteWorkContext {
        approved_matrix_selection: selection,
        selection_link: ModelRouteSelectionLink {
            candidate_set_id,
            caller_request_id,
            mapped_draft_node_index: 0,
            mapped_work_node_id: node_id,
            mapped_work_node_revision: 1,
        },
        role: caller("agent".into(), node_id),
        tool: caller("code".into(), node_id),
        data_class: caller("internal".into(), node_id),
        host_capabilities: ModelRouteHostCapabilities {
            schema: MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
            version: 1,
            capabilities: vec!["model-api".into()],
        }
        .fact()
        .unwrap(),
        remaining_budget_units: caller(10, node_id),
        available_latency_ms: caller(50, node_id),
    };
    if preparation == ModelRoutePreparation::UnknownWorkFacts {
        work.role = ModelRouteFact::Unknown;
    }
    let route = ModelRoute {
        id: "route-a".into(),
        provider: "configured-provider".into(),
        model: "configured-model".into(),
        effort: "medium".into(),
        enabled: true,
        allowed_matrix_choice_ids: vec!["choice-a".into()],
        allowed_roles: vec!["agent".into()],
        allowed_tools: vec!["code".into()],
        allowed_data_classes: vec!["internal".into()],
        required_host_capabilities: vec!["model-api".into()],
        minimum_budget_units: 10,
        minimum_latency_ms: 50,
    };
    let catalogue = ModelRouteCatalogue {
        schema: MODEL_ROUTE_CATALOGUE_SCHEMA.into(),
        version: 1,
        routes: vec![
            route.clone(),
            ModelRoute {
                id: "route-b".into(),
                ..route
            },
        ],
    };
    let eligible = catalogue.eligible(&work).unwrap();
    PreparedModelRouteRecommendation {
        workspace_id,
        request_key: "prepare-1".into(),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        advisory_config_revision: 2,
        work,
        catalogue: Some(catalogue),
        eligible: Some(eligible),
        preparation,
        routes: ModelRouteRecord {
            requested_route_id: Some("route-b".into()),
            recommended_route_id: None,
            observed_actual: None,
        },
    }
}

fn evidence(
    prepared: &PreparedModelRouteRecommendation,
    ids: &[&str],
) -> crate::ModelRouteSealedRankingEvidence {
    let request = ModelRouteRankingWireRequest::new(
        prepared.workspace_id,
        &prepared.request_key,
        &prepared.work,
        prepared.catalogue.as_ref().unwrap(),
        prepared.eligible.as_ref().unwrap(),
        "jev-adviser",
    )
    .unwrap();
    let attempted = crate::ModelRoutePreparedAttempt::new(request.clone()).unwrap();
    let raw_response = serde_json::json!({
        "schema": MODEL_ROUTE_RANKING_WIRE_SCHEMA,
        "binding_digest": request.binding_digest,
        "adviser_model": "jev-adviser",
        "outcome": {"kind":"ranked", "route_ids":ids},
    })
    .to_string()
    .into_bytes();
    crate::ModelRouteSealedRankingEvidence {
        permit: crate::ModelRouteSendPermit {
            attempt_id: Uuid::new_v4(),
            workspace_id: prepared.workspace_id,
            preparation_request_key: prepared.request_key.clone(),
            request_sha256: attempted.request_sha256.clone(),
        },
        attempted,
        response_sha256: tect_domain::model_route_wire_sha256(&raw_response),
        raw_response,
        outcome: ModelRouteRankingWireOutcome::Ranked {
            route_ids: ids.iter().map(|id| (*id).into()).collect(),
        },
    }
}

mod provider_cases;

#[tokio::test]
async fn ranked_recommendation_and_explicit_disposition_replay_without_dispatch() {
    let mut preparations = PreparationMemory {
        prepared: Some(prepared(ModelRoutePreparation::Prepared)),
    };
    let mut memory = Memory::default();
    let prepared = preparations.prepared.as_ref().unwrap();
    memory.evidence = Some(evidence(prepared, &["route-a", "route-b"]));
    let decision = DecideModelRouteRecommendation {
        id: Uuid::new_v4(),
        workspace_id: prepared.workspace_id,
        preparation_request_key: prepared.request_key.clone(),
        input: ModelRouteDecisionInput::Ranking(ModelRouteRanking {
            catalogue_digest: prepared.eligible.as_ref().unwrap().catalogue_digest.clone(),
            work_context_digest: prepared
                .eligible
                .as_ref()
                .unwrap()
                .work_context_digest
                .clone(),
            ranked_route_ids: vec!["route-a".into(), "route-b".into()],
        }),
    };
    let saved = decision
        .decide(&mut preparations, &mut memory)
        .await
        .unwrap();
    assert_eq!(
        saved.routes.recommended_route_id.as_deref(),
        Some("route-a")
    );
    assert_eq!(saved.routes.requested_route_id.as_deref(), Some("route-b"));
    assert_eq!(saved.routes.observed_actual, None);
    assert_eq!(memory.decision_writes, 1);
    assert_eq!(
        decision
            .decide(&mut preparations, &mut memory)
            .await
            .unwrap(),
        saved
    );
    assert_eq!(memory.decision_writes, 1);
    let differently_ranked = DecideModelRouteRecommendation {
        input: ModelRouteDecisionInput::Ranking(ModelRouteRanking {
            catalogue_digest: saved
                .prepared
                .eligible
                .as_ref()
                .unwrap()
                .catalogue_digest
                .clone(),
            work_context_digest: saved
                .prepared
                .eligible
                .as_ref()
                .unwrap()
                .work_context_digest
                .clone(),
            ranked_route_ids: vec!["route-b".into(), "route-a".into()],
        }),
        ..decision
    };
    assert_eq!(
        differently_ranked
            .decide(&mut preparations, &mut memory)
            .await,
        Err(Error::InputConflict)
    );
    assert_eq!(memory.decision_writes, 1);
    let conflicting_decision = DecideModelRouteRecommendation {
        id: Uuid::new_v4(),
        workspace_id: differently_ranked.workspace_id,
        preparation_request_key: differently_ranked.preparation_request_key.clone(),
        input: differently_ranked.input.clone(),
    };
    assert_eq!(
        conflicting_decision
            .decide(&mut preparations, &mut memory)
            .await,
        Err(Error::InputConflict)
    );

    let disposition = DispositionModelRouteRecommendation {
        id: Uuid::new_v4(),
        workspace_id: saved.prepared.workspace_id,
        decision_id: saved.id,
        actor_id: Uuid::new_v4(),
        action: ModelRouteDispositionAction::Accept,
        rationale: "I accept the recommendation only.".into(),
    };
    let recorded = disposition.record(&mut memory).await.unwrap();
    assert_eq!(recorded.action, ModelRouteDispositionAction::Accept);
    assert_eq!(disposition.record(&mut memory).await.unwrap(), recorded);
    assert_eq!(memory.disposition_writes, 1);
    let conflicting = DispositionModelRouteRecommendation {
        action: ModelRouteDispositionAction::Reject,
        ..disposition
    };
    assert_eq!(
        conflicting.record(&mut memory).await,
        Err(Error::InputConflict)
    );
    let conflicting_new_id = DispositionModelRouteRecommendation {
        id: Uuid::new_v4(),
        ..conflicting
    };
    assert_eq!(
        conflicting_new_id.record(&mut memory).await,
        Err(Error::InputConflict)
    );
    assert_eq!(memory.disposition_writes, 1);
}

#[tokio::test]
async fn no_route_and_abstain_are_explicit_and_cannot_be_dispositioned() {
    let mut preparations = PreparationMemory {
        prepared: Some(prepared(ModelRoutePreparation::UnknownWorkFacts)),
    };
    let mut memory = Memory::default();
    let p = preparations.prepared.as_ref().unwrap();
    let no_route = DecideModelRouteRecommendation {
        id: Uuid::new_v4(),
        workspace_id: p.workspace_id,
        preparation_request_key: p.request_key.clone(),
        input: ModelRouteDecisionInput::NoCall,
    };
    let saved = no_route
        .decide(&mut preparations, &mut memory)
        .await
        .unwrap();
    assert_eq!(
        saved.outcome,
        ModelRouteDecisionOutcome::NoRoute {
            reason: ModelRoutePreparation::UnknownWorkFacts
        }
    );
    assert_eq!(saved.routes.recommended_route_id, None);
    let disposition = DispositionModelRouteRecommendation {
        id: Uuid::new_v4(),
        workspace_id: saved.prepared.workspace_id,
        decision_id: saved.id,
        actor_id: Uuid::new_v4(),
        action: ModelRouteDispositionAction::Accept,
        rationale: "No route".into(),
    };
    assert_eq!(
        disposition.record(&mut memory).await,
        Err(Error::InputConflict)
    );

    preparations.prepared = Some(prepared(ModelRoutePreparation::Prepared));
    let p = preparations.prepared.as_ref().unwrap();
    let abstain = DecideModelRouteRecommendation {
        id: Uuid::new_v4(),
        workspace_id: p.workspace_id,
        preparation_request_key: p.request_key.clone(),
        input: ModelRouteDecisionInput::Abstain,
    };
    let saved = abstain
        .decide(&mut preparations, &mut memory)
        .await
        .unwrap();
    assert_eq!(
        saved.outcome,
        ModelRouteDecisionOutcome::Abstained {
            reason: ModelRouteAbstainReason::Explicit
        }
    );
    assert_eq!(saved.routes.observed_actual, None);
}

#[tokio::test]
async fn stale_or_unapproved_ranking_cannot_be_captured() {
    let mut preparations = PreparationMemory {
        prepared: Some(prepared(ModelRoutePreparation::Prepared)),
    };
    let mut decisions = Memory::default();
    let p = preparations.prepared.as_ref().unwrap();
    let input = DecideModelRouteRecommendation {
        id: Uuid::new_v4(),
        workspace_id: p.workspace_id,
        preparation_request_key: p.request_key.clone(),
        input: ModelRouteDecisionInput::Ranking(ModelRouteRanking {
            catalogue_digest: "0".repeat(64),
            work_context_digest: p.eligible.as_ref().unwrap().work_context_digest.clone(),
            ranked_route_ids: vec!["route-a".into()],
        }),
    };
    assert_eq!(
        input.decide(&mut preparations, &mut decisions).await,
        Err(Error::StaleRevision)
    );
    assert_eq!(decisions.decision_writes, 0);
    let mut unapproved = input;
    if let ModelRouteDecisionInput::Ranking(ranking) = &mut unapproved.input {
        ranking.catalogue_digest = preparations
            .prepared
            .as_ref()
            .unwrap()
            .eligible
            .as_ref()
            .unwrap()
            .catalogue_digest
            .clone();
        ranking.ranked_route_ids = vec!["unconfigured".into()];
    }
    assert_eq!(
        unapproved.decide(&mut preparations, &mut decisions).await,
        Err(Error::InvalidArguments)
    );
    assert_eq!(decisions.decision_writes, 0);
}
