use super::*;
use tect_domain::ModelRouteWireAbstainReason;

#[tokio::test]
async fn sealed_provider_abstain_is_not_labeled_owner_explicit() {
    let saved = prepared(ModelRoutePreparation::Prepared);
    let mut proof = evidence(&saved, &["route-a", "route-b"]);
    proof.outcome = ModelRouteRankingWireOutcome::Abstained {
        reason: ModelRouteWireAbstainReason::InsufficientEvidence,
    };
    proof.raw_response = serde_json::json!({
        "schema": MODEL_ROUTE_RANKING_WIRE_SCHEMA,
        "binding_digest": proof.attempted.request.binding_digest,
        "adviser_model": "jev-adviser",
        "outcome": {"kind":"abstained", "reason":"insufficient_evidence"},
    })
    .to_string()
    .into_bytes();
    proof.response_sha256 = tect_domain::model_route_wire_sha256(&proof.raw_response);
    let mut memory = Memory {
        evidence: Some(proof),
        ..Memory::default()
    };
    let mut preparations = PreparationMemory {
        prepared: Some(saved.clone()),
    };
    let decision = DecideModelRouteRecommendation {
        id: Uuid::new_v4(),
        workspace_id: saved.workspace_id,
        preparation_request_key: saved.request_key.clone(),
        input: ModelRouteDecisionInput::Abstain,
    };
    let result = decision
        .decide(&mut preparations, &mut memory)
        .await
        .unwrap();
    assert_eq!(
        result.outcome,
        ModelRouteDecisionOutcome::Abstained {
            reason: ModelRouteAbstainReason::ProviderInsufficientEvidence,
        }
    );
    assert_eq!(result.routes.observed_actual, None);
}

#[tokio::test]
async fn caller_rank_without_sealed_provider_evidence_is_denied() {
    let saved = prepared(ModelRoutePreparation::Prepared);
    let ranking = ModelRouteRanking {
        catalogue_digest: saved.eligible.as_ref().unwrap().catalogue_digest.clone(),
        work_context_digest: saved.eligible.as_ref().unwrap().work_context_digest.clone(),
        ranked_route_ids: vec!["route-a".into(), "route-b".into()],
    };
    let decision = DecideModelRouteRecommendation {
        id: Uuid::new_v4(),
        workspace_id: saved.workspace_id,
        preparation_request_key: saved.request_key.clone(),
        input: ModelRouteDecisionInput::Ranking(ranking),
    };
    let mut preparations = PreparationMemory {
        prepared: Some(saved.clone()),
    };
    let mut memory = Memory::default();
    assert_eq!(
        decision.decide(&mut preparations, &mut memory).await,
        Err(Error::Forbidden)
    );
    let mut forged = evidence(&saved, &["route-a", "route-b"]);
    forged.raw_response.push(b' ');
    memory.evidence = Some(forged);
    assert_eq!(
        decision.decide(&mut preparations, &mut memory).await,
        Err(Error::InputConflict)
    );
    assert_eq!(memory.decision_writes, 0);
}
