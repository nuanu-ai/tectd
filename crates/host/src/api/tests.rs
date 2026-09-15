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
fn public_surface_is_exactly_five_tools_and_fifty_four_registry_routes() {
    let definitions = definitions();
    assert_eq!(
        names(&definitions),
        BTreeSet::from(["command", "execute", "get_state", "help", "query"])
    );
    assert_eq!(routes().len(), 54);
    assert_eq!(
        routes()
            .iter()
            .filter(|route| route.tool == "query")
            .count(),
        16
    );
    assert_eq!(
        routes()
            .iter()
            .filter(|route| route.tool == "command")
            .count(),
        37
    );
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
    assert!(
        definitions["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|tool| tool["inputSchema"]["additionalProperties"] == false)
    );
}

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
    assert_eq!(slices["method_revision"], "2");
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
fn help_search_is_bounded_stable_filtered_and_bilingual() {
    let all = help(parse_help(json!({"mode":"search"})).unwrap()).unwrap();
    assert_eq!(all["total_matches"], 63);
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
