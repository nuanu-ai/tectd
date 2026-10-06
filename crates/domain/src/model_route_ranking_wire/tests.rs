use super::*;
use crate::{
    MODEL_ROUTE_CATALOGUE_SCHEMA, MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA, MatrixPlanningSelection,
    ModelRouteContextAuthority, ModelRouteSelectionLink,
};

fn observed<T>(value: T, node: Uuid) -> ModelRouteFact<T> {
    ModelRouteFact::Known {
        value,
        provenance: ModelRouteFactProvenance::OperatingEvidence {
            source_ref: "synthetic-test-observation".into(),
            content_digest: "e".repeat(64),
            observed_at_epoch_ms: 1,
            expires_at_epoch_ms: i64::MAX,
            work_node_id: node,
            work_node_revision: 1,
        },
    }
}

fn request() -> ModelRouteRankingWireRequest {
    let node = Uuid::new_v4();
    let work = ModelRouteWorkContext {
        approved_matrix_selection: MatrixPlanningSelection {
            task_id: Uuid::new_v4(),
            task_revision: 1,
            disposition_id: Uuid::new_v4(),
            selected_choice_id: "choice-a".into(),
            expected_input_digest: "a".repeat(64),
            expected_choice_set_digest: "b".repeat(64),
            expected_verification_digest: "c".repeat(64),
            mapped_draft_node_indices: vec![0],
        },
        selection_link: ModelRouteSelectionLink {
            candidate_set_id: Uuid::new_v4(),
            caller_request_id: Uuid::new_v4(),
            mapped_draft_node_index: 0,
            mapped_work_node_id: node,
            mapped_work_node_revision: 1,
        },
        context_authority: Some(ModelRouteContextAuthority {
            frozen_snapshot_id: Uuid::new_v4(),
            authority_schema: crate::MATRIX_REQUIREMENTS_SCHEMA.into(),
            requirements_semantic_digest: "d".repeat(64),
            operating_verification_digest: "c".repeat(64),
        }),
        role: observed("agent".into(), node),
        tool: observed("code".into(), node),
        data_class: observed("internal".into(), node),
        host_capabilities: ModelRouteHostCapabilities {
            schema: MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
            version: 1,
            capabilities: vec!["model-api".into()],
        }
        .fact()
        .unwrap(),
        remaining_budget_units: observed(20, node),
        available_latency_ms: observed(100, node),
    };
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
    ModelRouteRankingWireRequest::new(
        Uuid::new_v4(),
        "prepare-1",
        &work,
        &catalogue,
        &eligible,
        "jev-adviser",
    )
    .unwrap()
}

fn response(request: &ModelRouteRankingWireRequest, ids: &[&str]) -> Vec<u8> {
    serde_json::json!({
        "schema": MODEL_ROUTE_RANKING_WIRE_SCHEMA,
        "binding_digest": request.binding_digest,
        "adviser_model": "jev-adviser",
        "outcome": {"kind":"ranked","route_ids":ids}
    })
    .to_string()
    .into_bytes()
}

#[test]
fn exact_finite_rank_and_explicit_abstain_only() {
    let request = request();
    assert_eq!(request.binding.eligible_route_ids, ["route-a", "route-b"]);
    assert_eq!(request.binding.digest().unwrap(), request.binding_digest);
    let outcome =
        parse_model_route_ranking_response(&request, &response(&request, &["route-b", "route-a"]))
            .unwrap();
    assert_eq!(
        model_route_ranking_from_wire(&request, &outcome)
            .unwrap()
            .ranked_route_ids,
        ["route-b", "route-a"]
    );
    let abstain = serde_json::json!({
        "schema": MODEL_ROUTE_RANKING_WIRE_SCHEMA,
        "binding_digest": request.binding_digest,
        "adviser_model": "jev-adviser",
        "outcome": {"kind":"abstained","reason":"insufficient_evidence"}
    });
    let outcome =
        parse_model_route_ranking_response(&request, abstain.to_string().as_bytes()).unwrap();
    assert!(model_route_ranking_from_wire(&request, &outcome).is_none());
}

#[test]
fn invented_missing_duplicate_stale_and_malformed_are_rejected() {
    let request = request();
    for ids in [
        vec!["route-a"],
        vec!["route-a", "route-a"],
        vec!["route-a", "invented"],
        vec![],
    ] {
        assert!(parse_model_route_ranking_response(&request, &response(&request, &ids)).is_err());
    }
    let mut stale: serde_json::Value =
        serde_json::from_slice(&response(&request, &["route-a", "route-b"])).unwrap();
    stale["binding_digest"] = serde_json::json!("0".repeat(64));
    assert_eq!(
        parse_model_route_ranking_response(&request, stale.to_string().as_bytes()),
        Err(Error::InputConflict)
    );
    stale["binding_digest"] = serde_json::json!(request.binding_digest);
    stale["extra"] = serde_json::json!(true);
    assert_eq!(
        parse_model_route_ranking_response(&request, stale.to_string().as_bytes()),
        Err(Error::InvalidArguments)
    );
    let mut tampered = request.clone();
    tampered.binding.eligible_route_ids.pop();
    assert!(tampered.validate().is_err());
}
