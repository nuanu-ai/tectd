use super::*;
#[test]
fn only_intro_has_a_budget_and_json_is_not_mirrored() {
    for intro in INTROS {
        assert!(intro.len() <= 2000);
    }
    for error in [
        Error::StaleRevision,
        Error::InputPending,
        Error::ProgramIncomplete,
        Error::InputConflict,
        Error::WorkspaceNotOpen,
        Error::RequestTooLarge,
        Error::StorageUnavailable,
        Error::TransportUnavailable,
        Error::InvalidArguments,
        Error::Unauthorized,
    ] {
        assert!(error_intro(&error).len() <= 2000);
    }
    let large = "narrative".repeat(10_000);
    let response = success(json!({"large":large}));
    assert!(response.get("structuredContent").is_none());
    assert_eq!(response["content"].as_array().unwrap().len(), 3);
    let parsed: Value =
        serde_json::from_str(response["content"][1]["text"].as_str().unwrap()).unwrap();
    assert_eq!(parsed["large"], large);
    assert_eq!(response["content"][2]["type"], "text");
    assert_eq!(response["content"][2]["text"], RESPONSE_RULES);
    assert_eq!(
        encoded_len(&json!({"large":large})).unwrap(),
        serde_json::to_vec(&response).unwrap().len()
    );
    let mut without_rules = response.clone();
    without_rules["content"].as_array_mut().unwrap().pop();
    assert!(
        encoded_len(&json!({"large":large})).unwrap()
            > serde_json::to_vec(&without_rules).unwrap().len()
    );
}

#[test]
fn normal_and_fallback_failures_keep_json_and_rules_in_place() {
    for response in [failure(Error::Unauthorized, None), internal_failure()] {
        assert_eq!(response["isError"], true);
        assert_eq!(response["content"].as_array().unwrap().len(), 3);
        let payload: Value =
            serde_json::from_str(response["content"][1]["text"].as_str().unwrap()).unwrap();
        assert!(payload["error"]["code"].is_string());
        assert_eq!(response["content"][2]["type"], "text");
        assert_eq!(response["content"][2]["text"], RESPONSE_RULES);
    }
}

#[test]
fn promotion_refusal_returns_the_same_slice_owner_continuation() {
    let id = uuid::Uuid::new_v4();
    let arguments = json!({"request_id":id,"scope_id":id,"slice_id":id,
        "slice_revision":3,"qualification_reason":"Publish this bounded evidence.",
        "delivery_mode":"whole"});
    let value = try_failure_with_state(
        Error::KnowledgeLifecycleRequired,
        Some(("slice_pipeline_begin", &arguments)),
        None,
    )
    .unwrap();
    let value: Value = serde_json::from_str(value["content"][1]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        value["actions"][0]["arguments"]["route"],
        "knowledge.lifecycle"
    );
    assert_eq!(
        value["actions"][1]["arguments"]["route"],
        "knowledge.change_begin"
    );
    assert_eq!(
        value["actions"][1]["arguments"]["params"]["owner"]["kind"],
        "promotion_slice"
    );
    assert_eq!(
        value["actions"][1]["arguments"]["params"]["owner"]["slice_revision"],
        3
    );
}

#[test]
fn typed_refusal_is_additive_to_legacy_error_code() {
    let value = failure(Error::StaleRevision, None);
    let body: Value = serde_json::from_str(value["content"][1]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["error"]["code"], "stale_revision");
    assert_eq!(body["error"]["refusal"]["code"], "STALE_REVISION");
    assert_eq!(body["error"]["refusal"]["next_action"], "refresh");
    assert_eq!(body["error"]["refusal"]["required"], "revision");
}

fn failure_body(error: Error, name: &str, arguments: &Value) -> Value {
    let value = failure(error, Some((name, arguments)));
    serde_json::from_str(value["content"][1]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn invalid_known_routes_recommend_executable_route_specific_schema_help() {
    for (tool, route) in [
        ("command", "scope.candidates.begin"),
        ("query", "program.get"),
        ("execute", "setup.apply"),
    ] {
        let arguments = json!({"route":route,"params":{"invalid":true}});
        let body = failure_body(Error::InvalidArguments, tool, &arguments);
        assert_eq!(body["error"]["code"], "invalid_arguments");
        assert_eq!(body["error"]["refusal"]["code"], "INPUT_SCHEMA_INVALID");
        assert_eq!(
            body["error"]["refusal"]["next_action"],
            "correct_input_and_retry"
        );
        assert_eq!(body["error"]["refusal"]["required"], "schema_valid_input");
        assert_eq!(body["error"]["tool"], tool);
        assert_eq!(body["error"]["route"], route);
        assert!(body["error"]["route_contract"]["params_schema"].is_object());
        assert_eq!(body["recommended_action"], 0);
        let action = &body["actions"][0];
        assert_eq!(action["kind"], "ready_call");
        assert_eq!(action["tool"], "help");
        assert_eq!(
            action["arguments"],
            json!({"mode":"describe","tool":tool,"route":route})
        );
        assert_eq!(action["route_contract"]["route"], route);

        let request = crate::api::parse_help(action["arguments"].clone()).unwrap();
        let described = crate::api::help(request).unwrap();
        assert_eq!(described["kind"], "route");
        assert_eq!(described["tool"], tool);
        assert_eq!(described["route"], route);
        assert!(described["params_schema"].is_object());

        // One refusal carries the executable schema and example directly.
        assert!(action["route_contract"]["example"].is_object());
    }
}

#[test]
fn schema_refusal_reports_pointer_and_full_nested_candidate_contract() {
    let arguments = json!({"route":"scope.candidates.save","params":{"kind":"draft"}});
    let body = failure_body(
        Error::invalid_arguments_from("missing field `candidate_set_id`"),
        "command",
        &arguments,
    );
    assert_eq!(body["error"]["refusal"]["code"], "INPUT_SCHEMA_INVALID");
    assert_eq!(
        body["error"]["details"]["violation_code"],
        "required_field_missing"
    );
    assert_eq!(
        body["error"]["details"]["pointer"],
        "/params/candidate_set_id"
    );
    let schema = &body["error"]["route_contract"]["params_schema"];
    assert!(schema["oneOf"][0]["properties"]["draft"]["properties"]["candidates"]["items"]
        ["properties"]["coverage_goals"]
        .is_object());
    assert_eq!(
        body["error"]["route_contract"]["example"]["arguments"]["route"],
        "scope.candidates.save"
    );
}

#[test]
fn invalid_unknown_or_unrouted_calls_keep_state_fallback() {
    for (name, arguments) in [
        ("command", json!({"route":"unknown","params":{}})),
        ("unknown", json!({})),
    ] {
        let body = failure_body(Error::InvalidArguments, name, &arguments);
        assert_eq!(body["recommended_action"], 0);
        assert_eq!(
            body["actions"][0],
            json!({"kind":"ready_call","tool":"get_state","arguments":{}})
        );
    }
}
