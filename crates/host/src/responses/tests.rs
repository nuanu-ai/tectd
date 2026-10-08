use super::*;
#[test]
fn receipt_refusal_recovers_only_current_call_run_with_exact_receipt_diff() {
    for rule in ["WP6-SKILL-READ-01", "WP6-RESOURCE-READ-01"] {
        for id in [
            uuid::Uuid::new_v4().to_string(),
            "invalid".into(),
            uuid::Uuid::nil().to_string(),
        ] {
            let args = json!({"route":"slice.pipeline.phase.complete", "params":{"run_id":id,"phase_id":"phase","output":{"skill_reads":[],"resource_reads":[]}}});
            let error = Error::refused_at(
                tect_domain::RefusalCode::InvalidOutput,
                rule,
                "arguments.params.output.skill_reads",
                "pinned receipts",
                "missing",
                "supply_exact_phase_skill_reads",
                "exact_phase_skill_reads",
            );
            let response = failure(error, Some(("command", &args)));
            let body: Value =
                serde_json::from_str(response["content"][1]["text"].as_str().unwrap()).unwrap();
            assert_eq!(body["error"]["refusal"]["rule"], rule);
            let actions = body["actions"].as_array().unwrap();
            if uuid::Uuid::parse_str(&id).is_ok_and(|id| !id.is_nil()) {
                assert_eq!(actions.len(), 1);
                assert_eq!(actions[0]["arguments"]["params"]["run_id"], id);
                assert_eq!(actions[0]["arguments"]["params"]["view"], "receipt_diff");
            } else {
                assert!(actions.is_empty());
            }
            assert_eq!(response["content"][2]["text"], RESPONSE_FOOTER);
        }
    }
}

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
        Error::OperationTimeout,
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
    assert_eq!(response["content"][2]["text"], RESPONSE_FOOTER);
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
        assert_eq!(response["content"][2]["text"], RESPONSE_FOOTER);
    }
}

#[test]
fn operation_deadline_reads_existing_change_before_same_id_retry() {
    let change_id = uuid::Uuid::new_v4();
    let request_id = uuid::Uuid::new_v4();
    let arguments = json!({"route":"knowledge.change_phase_complete","params":{
        "change_id":change_id,"request_id":request_id,"phase_id":"kc-impact-plan"
    }});
    let response = failure(Error::OperationTimeout, Some(("command", &arguments)));
    let body: Value =
        serde_json::from_str(response["content"][1]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["error"]["code"], "operation_timeout");
    assert_eq!(
        body["actions"][0]["arguments"]["route"],
        "knowledge.lifecycle"
    );
    assert_eq!(
        body["actions"][0]["arguments"]["params"]["change_id"],
        json!(change_id)
    );
    assert!(
        response["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("same request ID")
    );

    let begin = json!({"route":"knowledge.change_begin","params":{"request_id":request_id}});
    let response = failure(Error::OperationTimeout, Some(("command", &begin)));
    let body: Value =
        serde_json::from_str(response["content"][1]["text"].as_str().unwrap()).unwrap();
    assert_ne!(body["actions"][0]["arguments"], begin);
    assert!(body["actions"][0]["arguments"].get("request_id").is_none());
    assert_eq!(body["recommended_action"], 0);

    let read = failure(Error::OperationTimeout, Some(("get_state", &json!({}))));
    assert!(
        read["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Retry the read")
    );
    assert!(
        !read["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("request ID")
    );
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
        assert!(body["error"].get("route_contract").is_none());
        assert_eq!(body["error"]["schema_help"]["arguments"]["route"], route);
        assert_eq!(body["recommended_action"], 0);
        let action = &body["actions"][0];
        assert_eq!(action["kind"], "ready_call");
        assert_eq!(action["tool"], "help");
        assert_eq!(
            action["arguments"],
            json!({"mode":"describe","tool":tool,"route":route})
        );
        assert!(action.get("route_contract").is_none());

        let request = crate::api::parse_help(action["arguments"].clone()).unwrap();
        let described = crate::api::help(request).unwrap();
        assert_eq!(described["kind"], "route");
        assert_eq!(described["tool"], tool);
        assert_eq!(described["route"], route);
        assert!(described["params_schema"].is_object());

        assert!(described["example"].is_object());
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
    let described = crate::api::help(
        crate::api::parse_help(body["error"]["schema_help"]["arguments"].clone()).unwrap(),
    )
    .unwrap();
    let schema = &described["params_schema"];
    assert!(schema["oneOf"][0]["properties"]["draft"]["properties"]["candidates"]["items"]
        ["properties"]["coverage_goals"]
        .is_object());
    assert_eq!(
        described["example"]["arguments"]["route"],
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

#[test]
fn state_conflict_retains_diagnostics_and_scoped_recovery_but_access_denial_does_not() {
    let scope = uuid::Uuid::new_v4();
    let args = json!({"route":"slice.candidates.save","params":{"scope_id":scope}});
    let error = Error::refused_at(
        tect_domain::RefusalCode::StateConflict,
        "SLICE-DRAFT-OPENED-PRESERVED",
        "/params/draft/nodes",
        "retain opened nodes",
        "omitted",
        "slice.candidates.context",
        "preserved_opened_nodes",
    );
    let result = super::failure(error, Some(("command", &args)));
    let body: Value = serde_json::from_str(result["content"][1]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["error"]["refusal"]["path"], "/params/draft/nodes");
    assert_eq!(
        body["actions"][0]["arguments"]["params"]["scope_id"],
        scope.to_string()
    );
    let denied = super::failure(Error::Forbidden, Some(("command", &args)));
    let denied: Value =
        serde_json::from_str(denied["content"][1]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        denied["error"]["refusal"],
        json!({"code":"AUTHORITY_REQUIRED"})
    );
    assert_eq!(denied["actions"], json!([]));
    assert!(denied["error"].get("route").is_none());
}
