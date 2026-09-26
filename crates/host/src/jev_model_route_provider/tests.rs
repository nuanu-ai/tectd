use super::*;
use serde_json::{Value, json};
use tect_domain::*;
use uuid::Uuid;

#[test]
fn duplicate_json_is_unusable_without_changing_observation() {
    let p = provider(config());
    let attempted = attempted(&p, 7);
    let valid = serde_json::to_string(&response()).unwrap();
    for raw in [
        valid.replace(
            "\"input_tokens\":20",
            "\"input_tokens\":999999,\"input_tokens\":0",
        ),
        valid.replace("\"usage\":", "\"usage\":{},\"usage\":"),
        valid.replace(
            "\"choice\":\"R0\"",
            "\"choice\":\"ABSTAIN\",\"choice\":\"R0\"",
        ),
    ] {
        let mut observed = observation(response(), Some(200));
        observed.raw = raw.into_bytes();
        let original = observed.clone();
        assert!(p.sealed_usage(&attempted, &observed).is_err());
        assert!(p.parse_sealed(&attempted, &observed).is_err());
        assert_eq!(observed, original);
    }
}

fn request(count: usize) -> ModelRouteRankingWireRequest {
    let id = Uuid::from_u128(1);
    fn caller<T>(value: T) -> ModelRouteFact<T> {
        ModelRouteFact::Known {
            value,
            provenance: ModelRouteFactProvenance::Caller {
                source_ref: "receipt#/draft/nodes/0/model_route_facts".into(),
                work_node_id: Uuid::from_u128(1),
                work_node_revision: 1,
            },
        }
    }
    let work = ModelRouteWorkContext {
        approved_matrix_selection: MatrixPlanningSelection {
            task_id: id,
            task_revision: 1,
            disposition_id: id,
            selected_choice_id: "choice-a".into(),
            expected_input_digest: "a".repeat(64),
            expected_choice_set_digest: "b".repeat(64),
            expected_verification_digest: "c".repeat(64),
            mapped_draft_node_indices: vec![0],
        },
        selection_link: ModelRouteSelectionLink {
            candidate_set_id: id,
            caller_request_id: id,
            mapped_draft_node_index: 0,
            mapped_work_node_id: id,
            mapped_work_node_revision: 1,
        },
        role: caller("agent".into()),
        tool: caller("code".into()),
        data_class: caller("internal".into()),
        host_capabilities: ModelRouteHostCapabilities {
            schema: MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
            version: 1,
            capabilities: vec!["model-api".into()],
        }
        .fact()
        .unwrap(),
        remaining_budget_units: caller(20),
        available_latency_ms: caller(100),
    };
    let routes = (0..count)
        .map(|i| ModelRoute {
            id: format!("route-{i:02}"),
            provider: "configured-provider".into(),
            model: format!("candidate-model-{i}"),
            effort: "medium".into(),
            enabled: true,
            allowed_matrix_choice_ids: vec!["choice-a".into()],
            allowed_roles: vec!["agent".into()],
            allowed_tools: vec!["code".into()],
            allowed_data_classes: vec!["internal".into()],
            required_host_capabilities: vec!["model-api".into()],
            minimum_budget_units: 10,
            minimum_latency_ms: 50,
        })
        .collect();
    let catalogue = ModelRouteCatalogue {
        schema: MODEL_ROUTE_CATALOGUE_SCHEMA.into(),
        version: 1,
        routes,
    };
    let eligible = catalogue.eligible(&work).unwrap();
    ModelRouteRankingWireRequest::new(id, "prepare-1", &work, &catalogue, &eligible, "jev-adviser")
        .unwrap()
}

fn config() -> JevModelRouteConfig {
    JevModelRouteConfig {
        profile: "local-test".into(),
        endpoint: Url::parse("http://127.0.0.1:1/v1/systemone").unwrap(),
        model: "jev-adviser".into(),
        timeout: Duration::from_millis(100),
        maximum_request_bytes: 100000,
        maximum_response_bytes: 16000,
    }
}
fn provider(config: JevModelRouteConfig) -> JevModelRouteProvider {
    JevModelRouteProvider::new(config, "local-test".into()).unwrap()
}
fn attempted(p: &JevModelRouteProvider, count: usize) -> ModelRoutePreparedAttempt {
    let request = request(count);
    let bytes = wire::prepare(&request, &p.binding_digest, p.config.maximum_request_bytes).unwrap();
    ModelRoutePreparedAttempt::native(request, bytes, MODEL_ROUTE_CHOICE_WIRE_VERSION.into())
        .unwrap()
}
fn response() -> Value {
    json!({"model":"jev-adviser","answers":{"model_route_order_v1":{"type":"choice","choice":"R0","probabilities":{"R0":0.3,"R1":0.2,"R2":0.15,"R3":0.1,"R4":0.09,"R5":0.07,"R6":0.05,"ABSTAIN":0.04},"confidence":0.01}},"usage":{"input_tokens":20,"output_tokens":30}})
}
fn observation(value: Value, status: Option<u16>) -> ModelRouteProviderObservation {
    ModelRouteProviderObservation {
        response_complete: Some(true), // Fixture explicitly models a complete HTTP frame.
        original_transport_context: None,
        raw: serde_json::to_vec(&value).unwrap(),
        http_status: status,
        input_tokens: None,
        output_tokens: None,
        elapsed_monotonic_ms: Some(5),
    }
}

#[test]
fn incomplete_or_historical_native_frame_has_no_usage_or_advice_even_if_json_valid() {
    let p = provider(config());
    let attempted = attempted(&p, 7);
    for complete in [None, Some(false)] {
        let mut observed = observation(response(), Some(200));
        observed.response_complete = complete;
        let original = observed.clone();
        assert!(p.sealed_usage(&attempted, &observed).is_err());
        assert!(p.parse_sealed(&attempted, &observed).is_err());
        assert_eq!(observed, original);
    }
    let complete_error = observation(response(), Some(500));
    assert_eq!(
        p.sealed_usage(&attempted, &complete_error)
            .unwrap()
            .input_tokens,
        Some(20)
    );
    assert!(p.parse_sealed(&attempted, &complete_error).is_err());
}

#[test]
fn full_seven_route_permutation_and_adviser_candidate_identity_separation() {
    let p = provider(config());
    let attempted = attempted(&p, 7);
    let body: Value = serde_json::from_slice(&attempted.request_bytes).unwrap();
    assert_eq!(body["model"], "jev-adviser");
    assert_eq!(
        body["state"]["request"]["eligible_routes"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    assert_eq!(
        body["state"]["request"]["eligible_routes"][0]["model"],
        "candidate-model-0"
    );
    assert_eq!(body["questions"].as_object().unwrap().len(), 1);
    assert_eq!(
        p.parse_sealed(&attempted, &observation(response(), Some(200)))
            .unwrap(),
        ModelRouteRankingWireOutcome::Ranked {
            route_ids: (0..7).map(|i| format!("route-{i:02}")).collect()
        }
    );
    let single = super::tests::attempted(&p, 1);
    let response = json!({"model":"jev-adviser","answers":{"model_route_order_v1":{"type":"choice","choice":"R0","probabilities":{"R0":0.51,"ABSTAIN":0.49}}}});
    assert_eq!(
        p.parse_sealed(&single, &observation(response, Some(200)))
            .unwrap(),
        ModelRouteRankingWireOutcome::Ranked {
            route_ids: vec!["route-00".into()]
        }
    );
}

#[test]
fn abstain_and_every_kind_of_tie_preserve_no_preference() {
    let p = provider(config());
    let attempted = attempted(&p, 7);
    for (choice, probabilities) in [
        (
            "ABSTAIN",
            json!({"R0":0.04,"R1":0.05,"R2":0.07,"R3":0.09,"R4":0.1,"R5":0.15,"R6":0.2,"ABSTAIN":0.3}),
        ),
        (
            "R0",
            json!({"R0":0.3,"R1":0.2,"R2":0.15,"R3":0.1,"R4":0.09,"R5":0.06,"R6":0.06,"ABSTAIN":0.04}),
        ),
        (
            "R0",
            json!({"R0":0.3,"R1":0.3,"R2":0.15,"R3":0.1,"R4":0.06,"R5":0.04,"R6":0.03,"ABSTAIN":0.02}),
        ),
        (
            "R0",
            json!({"R0":0.3,"R1":0.15,"R2":0.1,"R3":0.06,"R4":0.04,"R5":0.03,"R6":0.02,"ABSTAIN":0.3}),
        ),
    ] {
        let mut v = response();
        v["answers"]["model_route_order_v1"]["choice"] = json!(choice);
        v["answers"]["model_route_order_v1"]["probabilities"] = probabilities;
        assert_eq!(
            p.parse_sealed(&attempted, &observation(v, Some(200)))
                .unwrap(),
            ModelRouteRankingWireOutcome::Abstained {
                reason: ModelRouteWireAbstainReason::NoPreference
            }
        );
    }
}

#[test]
fn frozen_exact_request_and_current_config_identity_required() {
    let p = provider(config());
    assert!(p.available());
    let attempted = attempted(&p, 7);
    assert!(p.restore(&attempted).is_ok());
    assert_eq!(
        wire::prepare(&attempted.request, &p.binding_digest, 100000).unwrap(),
        attempted.request_bytes
    );
    for field in 0..3 {
        let mut c = config();
        match field {
            0 => c.endpoint = Url::parse("http://127.0.0.1:2/v1/systemone").unwrap(),
            1 => c.profile = "changed".into(),
            _ => c.model = "changed-adviser".into(),
        };
        assert!(provider(c).restore(&attempted).is_err());
    }
    let rotated = JevModelRouteProvider::new(config(), "rotated-local-test".into()).unwrap();
    assert!(rotated.restore(&attempted).is_ok());
    let mut changed = attempted.clone();
    changed.request_bytes.push(b' ');
    changed.request_sha256 = model_route_wire_sha256(&changed.request_bytes);
    assert!(p.restore(&changed).is_err());
    let mut changed = attempted.clone();
    changed.request_sha256 = "0".repeat(64);
    assert!(p.restore(&changed).is_err());
    let mut changed = attempted.clone();
    changed.adapter_identity = None;
    assert!(p.restore(&changed).is_err());
    for field in 0..3 {
        let mut c = config();
        match field {
            0 => c.model.clear(),
            1 => c.profile.clear(),
            _ => c.maximum_request_bytes = 0,
        };
        assert!(JevModelRouteProvider::new(c, "local-test".into()).is_err());
    }
}

#[test]
fn rejects_wrong_model_labels_type_selected_distribution_and_http_status() {
    let p = provider(config());
    let attempted = attempted(&p, 7);
    for (path, value) in [
        (vec!["model"], json!("candidate-model-0")),
        (
            vec!["answers", "model_route_order_v1", "type"],
            json!("score"),
        ),
        (
            vec!["answers", "model_route_order_v1", "choice"],
            json!("R1"),
        ),
        (
            vec!["answers", "model_route_order_v1", "choice"],
            json!("invented"),
        ),
        (
            vec!["answers", "model_route_order_v1", "probabilities", "R0"],
            json!(0.4),
        ),
        (
            vec!["answers", "model_route_order_v1", "probabilities", "R7"],
            json!(0),
        ),
        (
            vec!["answers", "model_route_order_v1", "confidence"],
            json!(1.01),
        ),
    ] {
        let mut v = response();
        let mut cursor = &mut v;
        for key in path {
            cursor = &mut cursor[key];
        }
        *cursor = value;
        assert!(
            p.parse_sealed(&attempted, &observation(v, Some(200)))
                .is_err()
        );
    }
    let mut missing = response();
    missing["answers"]["model_route_order_v1"]["probabilities"]
        .as_object_mut()
        .unwrap()
        .remove("R1");
    assert!(
        p.parse_sealed(&attempted, &observation(missing, Some(200)))
            .is_err()
    );
    for status in [None, Some(500), Some(302)] {
        assert!(
            p.parse_sealed(&attempted, &observation(response(), status))
                .is_err()
        );
    }
    let mut malformed = observation(response(), Some(200));
    malformed.raw = b"malformed {".to_vec();
    assert!(p.parse_sealed(&attempted, &malformed).is_err());
}

#[test]
fn pure_postseal_usage_missing_unknown_and_invalid_counters() {
    let p = provider(config());
    let attempted = attempted(&p, 7);
    assert_eq!(
        p.sealed_usage(&attempted, &observation(response(), Some(200)))
            .unwrap(),
        ModelRouteUsage {
            input_tokens: Some(20),
            output_tokens: Some(30)
        }
    );
    assert_eq!(
        p.sealed_usage(&attempted, &observation(json!({}), Some(200)))
            .unwrap(),
        ModelRouteUsage {
            input_tokens: None,
            output_tokens: None
        }
    );
    for value in [
        json!(-1),
        json!(1.5),
        json!("2"),
        json!(null),
        json!(u64::MAX),
    ] {
        assert!(
            p.sealed_usage(
                &attempted,
                &observation(json!({"usage":{"input_tokens":value}}), Some(200))
            )
            .is_err()
        );
    }
}
