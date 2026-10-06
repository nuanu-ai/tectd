use super::*;

fn state() -> tect_domain::WorkspaceState {
    serde_json::from_value(json!({"status":"uninitialized","workspace":null,"session":null,"selected_worktrees":[],"next_action":null,"programs":[],"next_after":null,"setup_context":null,"candidate_sets":[],"native_planning":[]})).unwrap()
}
#[test]
fn full_rules_bootstrap_and_real_help_are_byte_identical_with_short_default_footers() {
    assert_eq!(
        RESPONSE_RULES.as_bytes(),
        include_bytes!("../../response-rules.txt")
    );
    assert_eq!(RESPONSE_RULES.len(), 769);
    assert!(RESPONSE_FOOTER.len() <= 300);
    let opened = crate::workspace_output::opened(state(), 8192).unwrap();
    assert_eq!(
        opened["response_rules"].as_str().unwrap().as_bytes(),
        RESPONSE_RULES.as_bytes()
    );
    assert!(
        crate::workspace_output::workspace(state(), 8192)
            .unwrap()
            .get("response_rules")
            .is_none()
    );
    let call = crate::api::decode_public_call("help", json!({"text":"response-rules"})).unwrap();
    let crate::tools::Invocation::Help(request) =
        crate::tools::parse_invocation(call.name, call.arguments).unwrap()
    else {
        panic!("help")
    };
    let help = crate::planning_read::help(request, 8192).unwrap();
    assert_eq!(
        help["body"].as_str().unwrap().as_bytes(),
        RESPONSE_RULES.as_bytes()
    );
    for response in [
        success(opened),
        success(help),
        failure_bounded(Error::InvalidArguments, None, None, 8192).unwrap(),
        internal_failure_bounded(8192).unwrap(),
    ] {
        assert_eq!(response["content"][2]["text"], RESPONSE_FOOTER);
        assert!(serde_json::to_vec(&response).unwrap().len() <= 8192);
    }
    let error = crate::api::parse_help(json!({"query":"response-rules"}))
        .err()
        .unwrap();
    assert_eq!(
        error.refusal().unwrap().path.as_deref(),
        Some("arguments.query")
    );
    assert_eq!(
        error.refusal().unwrap().rule.as_deref(),
        Some("WP6-HELP-TEXT-SELECTOR")
    );
    println!(
        "rules_bytes={}; footer_bytes={}",
        RESPONSE_RULES.len(),
        RESPONSE_FOOTER.len()
    );
}
#[test]
fn rules_help_byte_windows_reassemble_losslessly_on_real_help_route() {
    let expected = serde_json::to_vec(
        &crate::api::help(crate::api::parse_help(json!({"text":"response-rules"})).unwrap())
            .unwrap(),
    )
    .unwrap();
    let mut args = json!({"text":"response-rules","limit_bytes":31});
    let mut assembled = Vec::new();
    loop {
        let value = crate::planning_read::help(crate::api::parse_help(args.clone()).unwrap(), 8192)
            .unwrap();
        assert!(encoded_len(&value).unwrap() <= 8192);
        assembled.extend_from_slice(value["text"].as_str().unwrap().as_bytes());
        if value["next_offset_bytes"].is_null() {
            break;
        }
        let action = &value["actions"][0];
        assert_eq!(action["tool"], "help");
        args = action["arguments"].clone();
        assert_eq!(args["text"], "response-rules");
        assert!(args.get("query").is_none());
        crate::api::decode_public_call("help", args.clone()).unwrap();
    }
    assert_eq!(assembled, expected);
}
#[test]
fn ambiguous_phase_failure_has_exact_current_recovery_and_no_fake_instruction() {
    let error = Error::Refused(Box::new(
        tect_domain::Refusal::new(tect_domain::RefusalCode::InputSchemaInvalid)
            .with_rule("WP6-INSTRUCTION-PHASE-AMBIGUOUS")
            .with_path("arguments.params.phase_id"),
    ));
    let id = uuid::Uuid::new_v4();
    let args =
        json!({"route":"slice.pipeline.instruction","params":{"run_id":id,"phase_id":"phase"}});
    let response = failure_bounded(error, Some(("query", &args)), None, 8192).unwrap();
    let value: Value =
        serde_json::from_str(response["content"][1]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        value["actions"][0]["arguments"]["params"],
        json!({"run_id":id,"view":"current"})
    );
    assert!(serde_json::to_vec(&response).unwrap().len() <= 8192);
}
