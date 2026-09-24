use super::*;
use serde_json::json;

#[test]
fn every_route_example_uses_the_authoritative_strict_decoder() {
    for spec in routes() {
        let call = decode_public_call(spec.tool, json!({"route":spec.route,"params":spec.example}));
        assert!(call.is_ok(), "{}: {call:?}", spec.route);

        let mut unknown = spec.example.clone();
        unknown["forged_authority"] = json!("denied");
        assert!(
            decode_public_call(spec.tool, json!({"route":spec.route,"params":unknown})).is_err(),
            "{} accepted an unknown parameter",
            spec.route
        );

        for field in required_fields(&spec.schema) {
            let mut missing = spec.example.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(
                decode_public_call(spec.tool, json!({"route":spec.route,"params":missing}))
                    .is_err(),
                "{} accepted missing {field}",
                spec.route
            );
        }

        let Some(properties) = spec.schema["properties"].as_object() else {
            continue;
        };
        for (field, property) in properties {
            let mut explicit_null = spec.example.clone();
            explicit_null[field] = Value::Null;
            let accepted = decode_public_call(
                spec.tool,
                json!({"route":spec.route,"params":explicit_null}),
            )
            .is_ok();
            let nullable = property["type"]
                .as_array()
                .is_some_and(|types| types.iter().any(|kind| kind == "null"));
            assert_eq!(
                accepted, nullable,
                "{} null behavior diverged for {field}",
                spec.route
            );
        }
    }
}

#[test]
fn routed_envelopes_and_optional_nulls_fail_closed() {
    for invalid in [
        json!({"route":"program.get"}),
        json!({"route":"program.get","params":{},"workspace_key":"forged"}),
        json!({"route":"program.get","params":[]}),
        json!({"route":"unknown","params":{}}),
    ] {
        assert!(decode_public_call("query", invalid).is_err());
    }
    for invalid in [
        json!({"route":"program.get","params":{"program_id":"00000000-0000-4000-8000-000000000001","after_input":null}}),
        json!({"route":"program.list","params":{"after":null}}),
        json!({"route":"source.list","params":{"limit":25,"after":null}}),
        json!({"route":"setup.get","params":{"setup_id":"00000000-0000-4000-8000-000000000001","after_input":null}}),
    ] {
        assert!(decode_public_call("query", invalid).is_err());
    }
    for legacy in ["open_workspace", "get_program", "read_skill", "apply_setup"] {
        assert!(decode_public_call(legacy, json!({})).is_err());
    }
}

#[test]
fn matrix_task_routes_are_discoverable_and_strict() {
    let definitions = definitions();
    assert_eq!(definitions["tools"].as_array().unwrap().len(), 5);
    for (tool, route) in [
        ("command", "task.source.record"),
        ("query", "task.source.get"),
    ] {
        let described =
            help(parse_help(json!({"mode":"describe","tool":tool,"route":route})).unwrap())
                .unwrap();
        assert_eq!(described["route"], route);
        assert!(decode_public_call(tool, described["example"]["arguments"].clone()).is_ok());
        let listed = help(parse_help(json!({"mode":"describe","tool":tool})).unwrap()).unwrap();
        assert!(listed["routes"].as_array().unwrap().contains(&json!(route)));
    }
    let spec = routes()
        .iter()
        .find(|spec| spec.route == "task.source.record")
        .unwrap();
    assert_eq!(
        spec.schema["properties"]["input"]["additionalProperties"],
        false
    );
    assert_eq!(
        spec.schema["properties"]["input"]["properties"]["mode"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    let entries = &spec.schema["properties"]["input"]["properties"]["envelope"]["properties"]["operational_facts"]
        ["oneOf"][2]["properties"]["entries"];
    assert_eq!(entries["maxItems"], 1024);
    let choice_set = &spec.schema["properties"]["choice_set"];
    assert_eq!(choice_set["additionalProperties"], false);
    assert_eq!(choice_set["properties"]["candidates"]["maxItems"], 5);
    assert_eq!(
        choice_set["properties"]["candidates"]["items"]["additionalProperties"],
        false
    );
    assert_eq!(
        spec.example["choice_set"]["candidates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let criticality_text = &spec.schema["properties"]["input"]["properties"]["criticality"]["oneOf"]
        [6]["properties"]["value"];
    assert_eq!(criticality_text["x-maxUtf8Bytes"], 256);
    assert_eq!(criticality_text["pattern"], "\\S");
    for (path, value) in [
        ("revision", json!(0)),
        ("expected_current_revision", json!(1)),
        ("task_id", json!("00000000-0000-0000-0000-000000000000")),
        ("request_id", json!("00000000-0000-0000-0000-000000000000")),
    ] {
        let mut params = spec.example.clone();
        params[path] = value;
        assert!(
            decode_public_call("command", json!({"route":spec.route,"params":params})).is_err(),
            "accepted bad {path}"
        );
    }
    for path in ["mode", "envelope", "intent", "operational_facts"] {
        let mut params = spec.example.clone();
        match path {
            "mode" => params["input"]["mode"]["forged"] = json!(true),
            "envelope" => params["input"]["envelope"]["forged"] = json!(true),
            "intent" => {
                params["input"]["intent"] = json!({"state":"known","value":{"kind":"production_hotfix","forged":true},"provenance":"source"})
            }
            _ => params["input"]["envelope"]["operational_facts"]["forged"] = json!(true),
        }
        assert!(
            decode_public_call("command", json!({"route":spec.route,"params":params})).is_err(),
            "accepted nested {path}"
        );
    }
}

#[test]
fn help_branches_are_strict_and_descriptions_come_from_registry() {
    let schema = definitions()["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "help")
        .unwrap()["inputSchema"]
        .clone();
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["oneOf"].as_array().unwrap().len(), 4);
    assert_eq!(schema["additionalProperties"], false);

    for invalid in [
        json!({"mode":"search","route":"program.get"}),
        json!({"mode":"search","method":"tectd-program"}),
        json!({"mode":"describe"}),
        json!({"mode":"describe","text":"program","tool":"command"}),
        json!({"mode":"describe","tool":"query","route":"program.begin"}),
        json!({"mode":"describe","method":"tectd-program","tool":"help"}),
        json!({"mode":"describe","method":"other"}),
        json!({"mode":"search","text":null}),
        json!({"mode":"search","tool":null}),
        json!({"mode":"describe","route":null,"tool":"query"}),
        json!({"mode":"describe","method":null}),
    ] {
        assert!(parse_help(invalid).is_err());
    }

    let HelpRequest::DescribeRoute(spec) = parse_help(json!({
        "mode":"describe","tool":"command","route":"program.begin"
    }))
    .unwrap() else {
        panic!("expected route description")
    };
    let described = help(HelpRequest::DescribeRoute(spec.clone())).unwrap();
    assert_eq!(described["params_schema"], spec.schema);
    assert_eq!(described["example"]["arguments"]["route"], spec.route);
    assert!(described["conditions"].as_str().unwrap().len() > 20);
    assert!(described["effects"].as_str().unwrap().len() > 20);
    assert!(described["retry"].as_str().unwrap().len() > 20);

    let method =
        help(parse_help(json!({"mode":"describe","method":"tectd-program"})).unwrap()).unwrap();
    assert_eq!(method["kind"], "method");
    assert!(method["body"].as_str().unwrap().contains("# TectD Program"));

    let candidates =
        help(parse_help(json!({"mode":"describe","method":"tectd-scope-candidates"})).unwrap())
            .unwrap();
    assert_eq!(candidates["method_revision"], "4");
    assert!(
        candidates["body"]
            .as_str()
            .unwrap()
            .contains("compact candidate history")
    );

    let slices =
        help(parse_help(json!({"mode":"describe","method":"tectd-slice-candidates"})).unwrap())
            .unwrap();
    assert_eq!(slices["method_revision"], "3");
    assert_eq!(
        slices["pipeline_catalog"]["pipelines"]
            .as_array()
            .unwrap()
            .len(),
        9
    );
    assert_eq!(slices["pipeline_catalog"]["executable"], true);
}

#[test]
fn phase_completion_help_allows_backend_derived_v07_proof() {
    let described = help(
        parse_help(json!({
            "mode":"describe",
            "tool":"command",
            "route":"slice.pipeline.phase.complete"
        }))
        .unwrap(),
    )
    .unwrap();
    let required = required_fields(&described["params_schema"]);
    assert!(!required.contains(&"consumed_outputs"));
    assert!(!required.contains(&"consumed_inputs"));
    assert!(
        described["params_schema"]["properties"]
            .get("consumed_outputs")
            .is_none()
    );
    assert!(
        described["params_schema"]["properties"]
            .get("consumed_inputs")
            .is_none()
    );
    let output_required = required_fields(&described["params_schema"]["properties"]["output"]);
    assert_eq!(output_required, vec!["producer_context_id"]);

    let arguments = described["example"]["arguments"].clone();
    assert!(arguments["params"].get("consumed_outputs").is_none());
    assert!(arguments["params"].get("consumed_inputs").is_none());
    assert!(arguments["params"]["output"].get("body").is_none());
    assert!(decode_public_call("command", arguments).is_ok());
}

#[test]
fn public_phase_completion_preserves_legacy_proof_and_classifies_other_unknowns() {
    let spec = routes()
        .iter()
        .find(|spec| spec.route == "slice.pipeline.phase.complete")
        .unwrap();

    for field in ["consumed_outputs", "consumed_inputs"] {
        let mut params = spec.example.clone();
        params[field] = json!([]);
        let call = decode_public_call("command", json!({"route":spec.route,"params":params}))
            .expect("known legacy receipt must pass routed deserialization");
        assert_eq!(call.arguments[field], json!([]));
    }

    let mut unknown_params = spec.example.clone();
    unknown_params["forged_authority"] = json!("denied");
    let error = decode_public_call(
        "command",
        json!({"route":spec.route,"params":unknown_params}),
    )
    .unwrap_err();
    let refusal = error.refusal().expect("typed input schema refusal");
    assert_eq!(error.code(), "invalid_arguments");
    assert_eq!(refusal.code, tect_domain::RefusalCode::InputSchemaInvalid);
    assert_eq!(refusal.rule.as_deref(), Some("WP6-SCHEMA-COMPLETE-01"));
    assert_eq!(refusal.path.as_deref(), Some("arguments.params"));
}

#[test]
fn internal_legacy_phase_actions_can_retain_backend_receipts() {
    let spec = routes()
        .iter()
        .find(|spec| spec.route == "slice.pipeline.phase.complete")
        .unwrap();
    let mut params = spec.example.clone();
    params["consumed_outputs"] = json!([]);
    params["consumed_inputs"] = json!([]);

    let mut action = needs_action(
        "needs_context",
        spec.internal,
        params,
        "context_input",
        json!({"fields":[]}),
    )
    .unwrap();
    attach_route_contract(&mut action).unwrap();

    assert!(action["arguments"]["params"]["consumed_outputs"].is_array());
    assert!(action["arguments"]["params"]["consumed_inputs"].is_array());
    assert!(
        action["route_contract"]["params_schema"]["properties"]
            .get("consumed_outputs")
            .is_none()
    );
    assert!(
        action["route_contract"]["params_schema"]["properties"]
            .get("consumed_inputs")
            .is_none()
    );
}

#[test]
fn help_search_is_bounded_stable_filtered_and_bilingual() {
    let all = help(parse_help(json!({"mode":"search"})).unwrap()).unwrap();
    assert_eq!(all["total_matches"], routes().len() + 9);
    assert_eq!(all["returned"], 25);
    assert_eq!(all["truncated"], true);
    assert_eq!(all["hits"][0]["tool"], "get_state");

    let russian = help(
        parse_help(json!({"mode":"search","text":"создать программу","tool":"command"})).unwrap(),
    )
    .unwrap();
    assert_eq!(russian["hits"].as_array().unwrap().len(), 1);
    assert_eq!(russian["hits"][0]["route"], "program.begin");

    let unknown =
        help(parse_help(json!({"mode":"search","text":"zz-no-such-route"})).unwrap()).unwrap();
    assert!(unknown["hits"].as_array().unwrap().is_empty());
    assert!(unknown["refine_search"].is_string());
}

fn required_fields(schema: &Value) -> Vec<&str> {
    schema["required"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

#[test]
fn action_kinds_distinguish_complete_calls_from_missing_values() {
    let ready = ready_action(
        "get_program",
        json!({
            "program_id":"00000000-0000-4000-8000-000000000001"
        }),
    )
    .unwrap();
    assert_eq!(ready["kind"], "ready_call");
    assert_eq!(ready["tool"], "query");
    assert_eq!(ready["arguments"]["route"], "program.get");
    assert_eq!(
        ready_action("get_program", json!({"program_id":"not-a-uuid"})),
        Err(Error::InternalInvariant)
    );

    let input = needs_action(
        "needs_input",
        "begin_program",
        json!({"request_id":"00000000-0000-4000-8000-000000000001"}),
        "input",
        json!({"fields":[{"path":"arguments.params.input","format":"exact input"}]}),
    )
    .unwrap();
    assert_eq!(input["kind"], "needs_input");
    assert_eq!(input["arguments"]["route"], "program.begin");
    assert_eq!(
        input["input"]["fields"][0]["path"],
        "arguments.params.input"
    );
}

#[test]
fn route_contract_enrichment_validates_help_as_a_meta_call() {
    let mut action = schema_help_action(
        "command",
        &json!({"route":"scope.candidates.begin","params":{}}),
    )
    .unwrap()
    .unwrap();
    attach_route_contract(&mut action).unwrap();
    assert_eq!(action["kind"], "ready_call");
    assert_eq!(action["tool"], "help");
    assert_eq!(action["route_contract"]["route"], "scope.candidates.begin");
    assert!(action["route_contract"]["params_schema"].is_object());

    action["arguments"]["route"] = json!("unknown");
    assert_eq!(
        attach_route_contract(&mut action),
        Err(Error::InternalInvariant)
    );

    let mut routed = ready_action(
        "get_program",
        json!({"program_id":"00000000-0000-4000-8000-000000000001"}),
    )
    .unwrap();
    attach_route_contract(&mut routed).unwrap();
    assert_eq!(routed["route_contract"]["tool"], "query");
    assert_eq!(routed["route_contract"]["route"], "program.get");
}
