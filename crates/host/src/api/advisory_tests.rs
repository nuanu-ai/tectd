use super::*;
use serde_json::json;
use std::collections::BTreeSet;

fn names(definitions: &Value) -> BTreeSet<&str> {
    definitions["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect()
}

#[test]
fn public_surface_is_exactly_five_tools_with_scope_advisory_request() {
    let definitions = definitions();
    assert_eq!(
        names(&definitions),
        BTreeSet::from(["command", "execute", "get_state", "help", "query"])
    );
    assert!(routes().iter().any(|route| route.tool == "query"));
    assert!(routes().iter().any(|route| route.tool == "command"));
    assert_eq!(
        routes()
            .iter()
            .filter(|route| route.tool == "execute")
            .count(),
        1
    );
    assert!(routes().iter().any(|route| {
        route.tool == "command" && route.route == "slice.pipeline.checkpoint.resolve"
    }));
    for (tool, route) in [
        ("command", "scope.candidates.delta"),
        ("query", "scope.candidates.delta.status"),
        ("command", "slice.pipeline.evidence_artifact.register"),
        ("command", "slice.pipeline.evidence_artifact.finalize"),
        ("query", "slice.pipeline.evidence_artifact.read"),
        ("query", "slice.pipeline.instruction"),
        ("command", "slice.pipeline.run.migrate"),
        ("query", "workspace.advisory.config"),
        ("query", "workspace.advisory.audit"),
        ("query", "scope.advisory.get"),
        ("query", "scope.advisory.card"),
        ("query", "scope.advisory.audit"),
        ("command", "workspace.advisory.configure"),
        ("command", "scope.advisory.request"),
        ("command", "scope.advisory.disposition"),
        ("command", "engineering.advisory.request"),
        ("command", "pipeline.recommendation.prepare"),
        ("command", "pipeline.recommendation.run"),
        ("query", "engineering.advisory.get"),
    ] {
        assert!(
            routes()
                .iter()
                .any(|spec| spec.tool == tool && spec.route == route),
            "missing intentional route {tool}:{route}"
        );
    }
    assert!(
        definitions["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|tool| tool["inputSchema"]["additionalProperties"] == false)
    );
}

#[test]
fn matrix_advisory_routes_use_exact_task_and_request_key() {
    let task_id = uuid::Uuid::new_v4();
    let request = json!({"task_id":task_id,"expected_task_revision":1,"request_key":"matrix-1","session_preference":"use_workspace","request_preference":"skip"});
    let get = json!({"task_id":task_id,"request_key":"matrix-1"});
    for (tool, route, params) in [
        ("command", "engineering.advisory.request", request.clone()),
        ("query", "engineering.advisory.get", get.clone()),
    ] {
        let spec = routes()
            .iter()
            .find(|spec| spec.tool == tool && spec.route == route)
            .unwrap();
        assert_eq!(spec.schema["additionalProperties"], false);
        assert!(decode_public_call(tool, json!({"route":route,"params":params})).is_ok());
        assert!(
            decode_public_call("execute", json!({"route":route,"params":spec.example})).is_err()
        );
    }
    for params in [
        json!({"task_id":task_id,"expected_task_revision":0,"request_key":"matrix-1"}),
        json!({"task_id":task_id,"expected_task_revision":1,"request_key":" matrix-1"}),
        json!({"task_id":task_id,"expected_task_revision":1,"request_key":"matrix-1","principal_id":task_id}),
    ] {
        assert!(
            decode_public_call(
                "command",
                json!({"route":"engineering.advisory.request","params":params})
            )
            .is_err()
        );
    }
    assert!(decode_public_call("query", json!({"route":"engineering.advisory.get","params":{"task_id":task_id,"request_key":"matrix-1","workspace_id":task_id}})).is_err());
}

#[test]
fn pipeline_prepare_is_strict_command_with_no_execution_authority() {
    let route = routes()
        .iter()
        .find(|spec| spec.route == "pipeline.recommendation.prepare")
        .unwrap();
    assert_eq!(route.tool, "command");
    assert_eq!(route.schema["additionalProperties"], false);
    assert_eq!(
        route.schema["properties"]["expected_candidate_set_revision"]["minimum"],
        2
    );
    assert!(route.effects.contains("No provider call"));
    let params = route.example.clone();
    assert_eq!(
        decode_public_call(
            "command",
            json!({"route":route.route,"params":params.clone()})
        )
        .unwrap()
        .name,
        "pipeline_recommendation_prepare"
    );
    assert!(decode_public_call("execute", json!({"route":route.route,"params":params})).is_err());
}

#[test]
fn pipeline_run_is_strict_command_with_one_opportunity() {
    let route = routes()
        .iter()
        .find(|spec| spec.route == "pipeline.recommendation.run")
        .unwrap();
    assert_eq!(route.tool, "command");
    assert_eq!(route.schema["additionalProperties"], false);
    assert!(route.effects.contains("does not open a Slice"));
    let id = uuid::Uuid::new_v4();
    let params = json!({"opportunity_id": id});
    assert_eq!(
        decode_public_call(
            "command",
            json!({"route":route.route,"params":params.clone()})
        )
        .unwrap()
        .name,
        "pipeline_recommendation_run"
    );
    assert!(decode_public_call("query", json!({"route":route.route,"params":params})).is_err());
    for params in [
        json!({}),
        json!({"opportunity_id":uuid::Uuid::nil()}),
        json!({"opportunity_id":id,"actor_id":id}),
    ] {
        assert!(
            decode_public_call("command", json!({"route":route.route,"params":params})).is_err()
        );
    }
}

#[test]
fn matrix_card_query_schema_requires_exact_revision_and_selected_full_card() {
    let route = routes()
        .iter()
        .find(|route| route.route == "scope.advisory.card")
        .unwrap();
    assert_eq!(route.tool, "query");
    assert_eq!(route.schema["additionalProperties"], false);
    assert_eq!(
        route.schema["properties"]["expected_task_revision"]["minimum"],
        1
    );
    let id = uuid::Uuid::new_v4();
    for params in [
        json!({"task_id":id,"expected_task_revision":1}),
        json!({"task_id":id,"expected_task_revision":1,"detail":"full","card_id":"EM02-SCOPE@0.1"}),
    ] {
        assert!(decode_public_call("query", json!({"route":route.route,"params":params})).is_ok());
    }
    for params in [
        json!({"task_id":id,"expected_task_revision":0}),
        json!({"task_id":id,"expected_task_revision":1,"detail":"full"}),
        json!({"task_id":id,"expected_task_revision":1,"detail":"all"}),
        json!({"task_id":id,"expected_task_revision":1,"card_id":"EM02-SCOPE"}),
        json!({"task_id":id,"expected_task_revision":1,"actor_id":id}),
    ] {
        assert!(decode_public_call("query", json!({"route":route.route,"params":params})).is_err());
    }
}

#[test]
fn public_disposition_schema_is_command_only_and_requires_cas_binding() {
    let route = routes()
        .iter()
        .find(|route| route.route == "scope.advisory.disposition")
        .unwrap();
    assert_eq!(route.tool, "command");
    assert_eq!(route.schema["additionalProperties"], false);
    assert_eq!(
        route.schema["properties"]["action"]["enum"],
        json!([
            "accept",
            "reject_all",
            "supersede_with_deterministic_choice"
        ])
    );
    for field in [
        "opportunity_id",
        "candidate_set_id",
        "request_id",
        "advice_id",
        "expected_revision",
        "action",
        "items",
        "rationale",
    ] {
        assert!(
            route.schema["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == field)
        );
    }
    let valid = route.example.clone();
    assert!(decode_public_call("command", json!({"route":route.route,"params":valid})).is_ok());
    let mut superseded = route.example.clone();
    superseded["action"] = json!("supersede_with_deterministic_choice");
    superseded["selected_id"] = superseded["items"][0]["alternative_id"].clone();
    superseded["items"][0]["state"] = json!("selected");
    assert!(
        decode_public_call("command", json!({"route":route.route,"params":superseded})).is_ok()
    );
    let mut forged = route.example.clone();
    forged["actor_id"] = json!(uuid::Uuid::new_v4());
    assert!(decode_public_call("command", json!({"route":route.route,"params":forged})).is_err());
    assert!(
        decode_public_call(
            "execute",
            json!({"route":route.route,"params":route.example})
        )
        .is_err()
    );
}

#[test]
fn help_tool_summaries_match_registry_route_counts() {
    for (tool, summary) in [
        (
            "query",
            format!(
                "Run one of {} named read-only routes.",
                routes().iter().filter(|spec| spec.tool == "query").count()
            ),
        ),
        (
            "command",
            format!(
                "Run one of {} named logical state-transition routes.",
                routes()
                    .iter()
                    .filter(|spec| spec.tool == "command")
                    .count()
            ),
        ),
        (
            "execute",
            format!(
                "Run one of {} explicit external-effect routes.",
                routes()
                    .iter()
                    .filter(|spec| spec.tool == "execute")
                    .count()
            ),
        ),
    ] {
        assert_eq!(describe_tool(tool)["description"], summary, "{tool}");
    }
}

#[test]
fn advisory_config_contract_rejects_half_config_and_bounds_identifiers() {
    for params in [
        json!({
            "expected_revision": 0,
            "mode": "optional",
            "provider_profile_ref": {"id": "jev-production"}
        }),
        json!({
            "expected_revision": 0,
            "mode": "optional",
            "model_configuration": {"model": "jev-advisory-v1"}
        }),
    ] {
        assert!(
            decode_public_call(
                "command",
                json!({"route":"workspace.advisory.configure","params":params})
            )
            .is_err()
        );
    }

    let schema = routes()
        .iter()
        .find(|spec| spec.route == "workspace.advisory.configure")
        .unwrap()
        .schema
        .clone();
    assert_eq!(schema["oneOf"].as_array().unwrap().len(), 2);
    assert_eq!(
        schema["properties"]["provider_profile_ref"]["oneOf"][0]["properties"]["id"]["maxLength"],
        256
    );
    assert_eq!(
        schema["properties"]["model_configuration"]["oneOf"][0]["properties"]["model"]["pattern"],
        r"^[^\s\u0000](?:[^\u0000]*[^\s\u0000])?$"
    );
}

#[test]
fn slice_zero_advisory_help_is_read_only_and_does_not_advertise_send() {
    for route_name in [
        "workspace.advisory.audit",
        "scope.advisory.get",
        "scope.advisory.audit",
    ] {
        let route = routes()
            .iter()
            .find(|route| route.route == route_name)
            .expect("Packet D route");
        assert_eq!(route.tool, "query");
        let contract = format!(
            "{} {} {} {}",
            route.summary, route.conditions, route.effects, route.retry
        )
        .to_lowercase();
        assert!(!contract.contains("initiate a provider"));
        assert!(!contract.contains("send to jev"));
    }
    let request = routes()
        .iter()
        .find(|route| route.route == "scope.advisory.request")
        .unwrap();
    assert_eq!(request.tool, "command");
    assert_eq!(request.schema["additionalProperties"], false);
    assert!(
        request.schema["properties"]
            .get("session_preference")
            .is_none()
    );
    assert!(request.schema["properties"].get("actor_id").is_none());
    assert!(request.schema["properties"].get("tenant_id").is_none());
    assert_eq!(
        request.schema["properties"]["authored_scope_set"]["properties"]["alternatives"]["maxItems"],
        100
    );
}
