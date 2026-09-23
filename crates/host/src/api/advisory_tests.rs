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
        ("query", "scope.advisory.audit"),
        ("command", "workspace.advisory.configure"),
        ("command", "scope.advisory.request"),
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
    for unavailable in ["scope.advisory.disposition", "scope.advisory.card"] {
        assert!(routes().iter().all(|spec| spec.route != unavailable));
    }
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
