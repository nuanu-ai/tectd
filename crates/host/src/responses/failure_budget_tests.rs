use super::*;
fn body(value: &Value) -> Value {
    serde_json::from_str(value["content"][1]["text"].as_str().unwrap()).unwrap()
}
fn size(value: &Value) -> usize {
    serde_json::to_vec(value).unwrap().len()
}
#[test]
fn actual_failure_envelopes_preserve_normal_and_honestly_omit_oversize_diagnostics() {
    let run = uuid::Uuid::new_v4();
    let args = json!({"route":"slice.pipeline.context","params":{"run_id":run,"view":"invalid"}});
    let error = crate::api::decode_public_call("query", args.clone())
        .unwrap_err()
        .normalize_pipeline_refusal(
            "QUERY-02",
            "arguments.params.view",
            "known view",
            "correct_input_and_retry",
            "schema_valid_input",
        );
    let value = failure_bounded(error, Some(("query", &args)), None, 8192).unwrap();
    assert!(size(&value) <= 8192);
    assert!(body(&value)["error"]["details"].is_object());
    let pointer = format!("/params/{}", "界".repeat(10000));
    let value = failure_bounded(
        Error::invalid_arguments_at("unknown field", pointer.clone()),
        Some(("query", &args)),
        None,
        8192,
    )
    .unwrap();
    let data = body(&value);
    assert_eq!(data["error"]["code"], "invalid_arguments");
    assert_eq!(data["error"]["diagnostic_delivery"]["complete"], false);
    assert!(
        data["error"]["diagnostic_delivery"]["omitted"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["field"] == "/details/pointer" && v["utf8_bytes"] == pointer.len())
    );
    assert!(data["error"]["details"].get("pointer").is_none());
    eprintln!(
        "failure giant Unicode pointer full MCP bytes={}",
        size(&value)
    );
    let smaller = failure_bounded(
        Error::invalid_arguments_at("unknown field", pointer),
        None,
        None,
        1600,
    )
    .unwrap();
    assert!(size(&smaller) <= 1600);
    eprintln!("failure smaller capacity full MCP bytes={}", size(&smaller));
    assert!(matches!(
        failure_bounded(Error::InvalidArguments, None, None, 1),
        Err(FailureBuildError::EnvelopeCannotFit)
    ));
    assert!(internal_failure_bounded(1).is_none());
}
#[test]
fn reload_uses_only_known_top_level_non_nil_identities_and_access_never_leaks() {
    let id = uuid::Uuid::new_v4();
    for args in [
        json!({"route":"program.get","params":{"program_id":id}}),
        json!({"route":"slice.pipeline.context","params":{"run_id":id}}),
    ] {
        let value =
            failure_bounded(Error::StaleRevision, Some(("query", &args)), None, 8192).unwrap();
        assert!(
            body(&value)["actions"][0]["arguments"]["params"]
                .to_string()
                .contains(&id.to_string())
        );
    }
    for args in [
        json!({"route":"program.get","params":{"program_id":uuid::Uuid::nil()}}),
        json!({"route":"program.get","params":{"nested":{"program_id":id}}}),
        json!({"route":"unknown","params":{"program_id":id}}),
    ] {
        let value =
            failure_bounded(Error::StaleRevision, Some(("query", &args)), None, 8192).unwrap();
        assert!(
            !body(&value)["actions"]
                .to_string()
                .contains(&id.to_string())
        );
    }
    let args = json!({"route":"program.get","params":{"program_id":id,"secret":"protected"}});
    let error = Error::Unauthorized.normalize_pipeline_refusal(
        "secret-rule",
        "secret-path",
        "secret-expected",
        "secret-next",
        "secret-required",
    );
    let value = failure_bounded(
        error,
        Some(("query", &args)),
        Some(&json!({"secret":"protected"})),
        8192,
    )
    .unwrap();
    assert!(body(&value)["actions"].as_array().unwrap().is_empty());
    assert!(!value.to_string().contains("secret"));
}
#[test]
fn giant_transient_replay_and_state_never_echo_or_forge_modified_ready_call() {
    let id = uuid::Uuid::new_v4();
    let huge = "body-marker-界".repeat(10000);
    let args = json!({"route":"program.save","params":{"program_id":id,"body":huge}});
    let value = failure_bounded(
        Error::StorageUnavailable,
        Some(("command", &args)),
        Some(&json!({"body":huge})),
        8192,
    )
    .unwrap();
    assert!(size(&value) <= 8192);
    assert!(!value.to_string().contains("body-marker"));
    for action in body(&value)["actions"].as_array().unwrap() {
        assert_ne!(action["arguments"]["route"], "program.save");
    }
    eprintln!("failure huge transient full MCP bytes={}", size(&value));
}
#[test]
fn actual_route_schema_and_completion_semantics_remain_exact_without_inline_contracts() {
    let id = uuid::Uuid::new_v4();
    let instruction = json!({"route":"slice.pipeline.instruction","params":{"run_id":id,"phase_id":"phase","instruction_digest":"invalid","refresh":false}});
    let error = crate::api::decode_public_call("query", instruction.clone()).unwrap_err();
    let expected = error
        .argument_diagnostic()
        .map(|d| serde_json::to_value(d).unwrap());
    let value = failure_bounded(error, Some(("query", &instruction)), None, 8192).unwrap();
    if let Some(expected) = expected {
        assert_eq!(body(&value)["error"]["details"], expected);
    }
    assert!(body(&value)["error"].get("route_contract").is_none());
    let args = json!({"route":"slice.pipeline.phase.complete","params":{"run_id":id}});
    let error = Error::refused_at(
        tect_domain::RefusalCode::EvidenceMissing,
        "COMPLETE-01",
        "arguments.params.output.evidence",
        "evidence",
        "missing",
        "supply_evidence",
        "evidence",
    );
    let value = failure_bounded(error, Some(("command", &args)), None, 8192).unwrap();
    assert_eq!(body(&value)["error"]["refusal"]["rule"], "COMPLETE-01");
    assert_eq!(
        body(&value)["error"]["refusal"]["path"],
        "arguments.params.output.evidence"
    );
    assert!(!value.to_string().contains("params_schema"));
    let value = failure_bounded(
        Error::invalid_arguments_at("invalid field", "x".repeat(6000)),
        None,
        None,
        8192,
    )
    .unwrap();
    assert!(body(&value)["error"].get("diagnostic_delivery").is_none());
    assert!(size(&value) <= 8192);
    eprintln!(
        "failure intact 6000-byte diagnostic full MCP bytes={}",
        size(&value)
    );
}

#[test]
fn giant_unknown_field_from_actual_instruction_parser_retains_source_diagnostic() {
    let mut params = json!({"run_id":uuid::Uuid::new_v4(),"instruction_id":"instruction","version":"version","digest":"a".repeat(64),"refresh":true});
    params[format!("unexpected-{}", "界".repeat(9000))] = json!(true);
    let args = json!({"route":"slice.pipeline.instruction","params":params});
    let error = crate::api::decode_public_call("query", args.clone()).unwrap_err();
    let expected = serde_json::to_value(error.argument_diagnostic().unwrap()).unwrap();
    let value = failure_bounded(error, Some(("query", &args)), None, 8192).unwrap();
    assert!(size(&value) <= 8192);
    assert_eq!(body(&value)["error"]["code"], "invalid_arguments");
    assert_eq!(body(&value)["error"]["details"], expected);
    eprintln!(
        "failure actual parser giant field full MCP bytes={}",
        size(&value)
    );
}

#[test]
fn setup_failure_recovery_never_carries_inline_route_contracts() {
    let args = json!({"setup_id":uuid::Uuid::new_v4()});
    let value =
        failure_bounded(Error::StaleRevision, Some(("get_setup", &args)), None, 8192).unwrap();
    assert!(!value.to_string().contains("route_contract"));
    let value = failure_bounded(
        Error::invalid_arguments_at("invalid", "x".repeat(10000)),
        Some(("get_setup", &args)),
        None,
        8192,
    )
    .unwrap();
    assert!(!value.to_string().contains("route_contract"));
}

#[test]
fn knowledge_reload_preserves_both_registered_families_for_internal_and_public_calls() {
    let id = uuid::Uuid::new_v4();
    for (internal, expected, expected_params) in [
        (
            "knowledge_change",
            "knowledge.change",
            json!({"change_id":id}),
        ),
        (
            "knowledge_change_review",
            "knowledge.change",
            json!({"change_id":id}),
        ),
        (
            "knowledge_lifecycle",
            "knowledge.lifecycle",
            json!({"change_id":id,"view":"current"}),
        ),
        (
            "knowledge_change_phase_complete",
            "knowledge.lifecycle",
            json!({"change_id":id,"view":"current"}),
        ),
        (
            "knowledge_change_begin",
            "knowledge.lifecycle",
            json!({"change_id":id,"view":"current"}),
        ),
        (
            "knowledge_change_record_input",
            "knowledge.lifecycle",
            json!({"change_id":id,"view":"current"}),
        ),
        (
            "knowledge_change_commit",
            "knowledge.lifecycle",
            json!({"change_id":id,"view":"current"}),
        ),
        (
            "knowledge_change_settle_effects",
            "knowledge.lifecycle",
            json!({"change_id":id,"view":"current"}),
        ),
    ] {
        let selector = crate::api::schema_help_action(internal, &json!({}))
            .unwrap()
            .unwrap();
        let tool = selector["arguments"]["tool"].as_str().unwrap();
        let args = json!({"route":selector["arguments"]["route"],"params":{"change_id":id}});
        for (name, arguments) in [(internal, json!({"change_id":id})), (tool, args)] {
            let value = failure_bounded(Error::StaleRevision, Some((name, &arguments)), None, 8192)
                .unwrap();
            let action = &body(&value)["actions"][0];
            assert_eq!(action["arguments"]["route"], expected);
            assert_eq!(action["arguments"]["params"], expected_params);
            let decoded = crate::api::decode_public_call(
                action["tool"].as_str().unwrap(),
                action["arguments"].clone(),
            )
            .unwrap();
            assert_eq!(
                decoded.name,
                if expected == "knowledge.lifecycle" {
                    "knowledge_lifecycle"
                } else {
                    "knowledge_change"
                }
            );
        }
    }
}
