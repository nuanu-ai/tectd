use super::*;

#[test]
fn definition_header_fields_and_priority_are_typed() {
    assert!(parse_definition(definition()).validate().is_ok());
    for (field, changed, suffix) in [
        ("version", json!(" "), "VERSION"),
        ("digest", json!(" "), "DIGEST"),
        ("completion_contract", json!(" "), "COMPLETION-CONTRACT"),
        ("escalation_contract", json!(" "), "ESCALATION-CONTRACT"),
        ("phases", json!([]), "PHASES"),
        ("default_mode", json!("whole"), "DEFAULT-MODE"),
    ] {
        let mut value = definition();
        value[field] = changed;
        assert_failure(
            parse_definition(value).validate(),
            &format!("WP6-DEFINITION-{suffix}"),
            &format!("pipeline_definition.{field}"),
        );
    }
    let mut value = definition();
    value["version"] = json!("");
    value["digest"] = json!("");
    value["overview"]["body"] = json!("");
    assert_failure(
        parse_definition(value).validate(),
        "WP6-DEFINITION-VERSION",
        "pipeline_definition.version",
    );
}
#[test]
fn instruction_fields_origins_and_all_collection_indexes_are_typed() {
    for collection in ["overview", "instructions", "skills", "resources"] {
        for (field, changed, suffix) in [
            ("id", json!(" "), "ID"),
            ("version", json!(" "), "VERSION"),
            ("digest", json!(" "), "DIGEST"),
            ("body", json!(" "), "BODY"),
            ("origin_refs", json!([]), "ORIGINS"),
            ("origin_refs", json!(["source", " "]), "ORIGIN-BLANK"),
        ] {
            let mut value = definition();
            let path = if collection == "overview" {
                value["overview"][field] = changed;
                "pipeline_definition.overview".to_string()
            } else {
                value["phases"][1][collection] = json!([instruction(), instruction()]);
                value["phases"][1][collection][1][field] = changed;
                format!("pipeline_definition.phases[1].{collection}[1]")
            };
            let path_field = if suffix == "ORIGIN-BLANK" {
                "origin_refs[1]"
            } else {
                field
            };
            assert_failure(
                parse_definition(value).validate(),
                &format!("WP6-INSTRUCTION-{suffix}"),
                &format!("{path}.{path_field}"),
            );
        }
    }
    let mut value = definition();
    value["overview"]["id"] = json!("");
    value["overview"]["body"] = json!("");
    assert_failure(
        parse_definition(value).validate(),
        "WP6-INSTRUCTION-ID",
        "pipeline_definition.overview.id",
    );
    for collection in ["instructions", "skills", "resources"] {
        let mut value = definition();
        value["version"] = json!("0.7-any");
        let mut invalid = instruction();
        invalid["id"] = json!("");
        invalid["body"] = json!("界".repeat(1366));
        value["phases"][1][collection] = json!([instruction(), invalid]);
        let error = parse_definition(value).validate().unwrap_err();
        let refusal = error.refusal().unwrap();
        assert_eq!(refusal.code, RefusalCode::PayloadTooLarge);
        assert_eq!(refusal.rule.as_deref(), Some("WP6-INSTRUCTION-SIZE-01"));
        assert_eq!(
            refusal.path.as_deref(),
            Some(format!("pipeline_definition.phases[1].{collection}[1].body").as_str())
        );
        assert!(!serde_json::to_string(&error).unwrap().contains("界"));
    }
}
#[test]
fn phase_fields_duplicates_blank_limits_and_membership_preserve_order() {
    for (field, changed, suffix, path_field) in [
        ("id", json!(" "), "ID", "id"),
        ("title", json!(" "), "TITLE", "title"),
        (
            "output_contract",
            json!(" "),
            "OUTPUT-CONTRACT",
            "output_contract",
        ),
        ("ordinal", json!(1), "ORDINAL", "ordinal"),
        ("id", json!("first"), "ID-DUPLICATE", "id"),
        (
            "required_fields",
            json!(["", " ", ""]),
            "REQUIRED-FIELD-DUPLICATE",
            "required_fields[2]",
        ),
        (
            "allowed_verdicts",
            json!(["PASS", "PASS"]),
            "ALLOWED-VERDICT-DUPLICATE",
            "allowed_verdicts[1]",
        ),
        (
            "required_dispositions",
            json!(["D", "D"]),
            "REQUIRED-DISPOSITION-DUPLICATE",
            "required_dispositions[1]",
        ),
        (
            "allowed_dispositions",
            json!(["D", "D"]),
            "ALLOWED-DISPOSITION-DUPLICATE",
            "allowed_dispositions[1]",
        ),
        (
            "verdict_routes",
            json!([route("PASS"), route("PASS")]),
            "VERDICT-ROUTE-DUPLICATE",
            "verdict_routes[1]",
        ),
        (
            "required_fields",
            json!(["ok", " "]),
            "REQUIRED-FIELD-BLANK",
            "required_fields[1]",
        ),
        (
            "allowed_verdicts",
            json!(["PASS", " "]),
            "ALLOWED-VERDICT-BLANK",
            "allowed_verdicts[1]",
        ),
        (
            "required_dispositions",
            json!(["ok", " "]),
            "REQUIRED-DISPOSITION-BLANK",
            "required_dispositions[1]",
        ),
        (
            "allowed_dispositions",
            json!(["ok", " "]),
            "ALLOWED-DISPOSITION-BLANK",
            "allowed_dispositions[1]",
        ),
        (
            "verdict_routes",
            json!([]),
            "VERDICT-ROUTES-MISSING",
            "verdict_routes",
        ),
        (
            "allowed_verdicts",
            json!(["PASS", "OTHER"]),
            "VERDICT-COVERAGE",
            "allowed_verdicts[1]",
        ),
    ] {
        let mut value = definition();
        value["phases"][1][field] = changed;
        assert_failure(
            parse_definition(value).validate(),
            &format!("WP6-PHASE-{suffix}"),
            &format!("pipeline_definition.phases[1].{path_field}"),
        );
    }
    let mut value = definition();
    value["phases"][1]["instructions"] = json!([]);
    value["phases"][1]["resources"] = json!([instruction()]);
    assert_failure(
        parse_definition(value).validate(),
        "WP6-PHASE-INSTRUCTION-OR-SKILL",
        "pipeline_definition.phases[1].instructions",
    );
    let mut value = definition();
    value["phases"][1]["disposition_required"] = json!(true);
    assert_failure(
        parse_definition(value).validate(),
        "WP6-PHASE-DISPOSITION-REQUIRED",
        "pipeline_definition.phases[1].allowed_dispositions",
    );
    let mut value = definition();
    value["phases"][1]["allowed_dispositions"] = json!(["D"]);
    value["phases"][1]["required_dispositions"] = json!(["X"]);
    assert_failure(
        parse_definition(value).validate(),
        "WP6-PHASE-REQUIRED-DISPOSITION-MEMBERSHIP",
        "pipeline_definition.phases[1].required_dispositions[0]",
    );
    let mut value = definition();
    value["phases"][1]["required_fields"] = json!(["1", "2", "3", "4", "5", "6", "7", "8", "9"]);
    assert!(parse_definition(value.clone()).validate().is_ok());
    value["version"] = json!("0.7.9");
    assert_failure(
        parse_definition(value).validate(),
        "WP6-PHASE-REQUIRED-FIELD-LIMIT",
        "pipeline_definition.phases[1].required_fields",
    );
    assert_failure(
        phase_ordinal(usize::MAX).map(|_| ()),
        "WP6-PHASE-ORDINAL-RANGE",
        &format!("pipeline_definition.phases[{}].ordinal", usize::MAX),
    );
    assert_eq!(phase_ordinal(1), Ok(2));
}
#[test]
fn route_fields_revisit_membership_dispositions_transitions_and_backwards_are_typed() {
    for (field, changed, suffix, path_field) in [
        ("verdict", json!(" "), "VERDICT-BLANK", "verdict"),
        ("verdict", json!("UNKNOWN"), "VERDICT-MEMBERSHIP", "verdict"),
        (
            "dispositions",
            json!(["D", "D"]),
            "DISPOSITION-DUPLICATE",
            "dispositions[1]",
        ),
        (
            "revisit_to",
            json!(["first", "first"]),
            "REVISIT-DUPLICATE",
            "revisit_to[1]",
        ),
        (
            "revisit_to",
            json!(["unknown"]),
            "REVISIT-MEMBERSHIP",
            "revisit_to[0]",
        ),
        (
            "dispositions",
            json!([" "]),
            "DISPOSITION-BLANK",
            "dispositions[0]",
        ),
    ] {
        let mut value = route_definition();
        value["phases"][1]["verdict_routes"][1][field] = changed;
        if field == "verdict" {
            value["phases"][1]["allowed_verdicts"] = json!(["PASS"]);
        }
        assert_failure(
            parse_definition(value).validate(),
            &format!("WP6-PHASE-ROUTE-{suffix}"),
            &format!("pipeline_definition.phases[1].verdict_routes[1].{path_field}"),
        );
    }
    let mut value = route_definition();
    value["phases"][1]["verdict_routes"][1]["revisit_to"] = json!(["first"]);
    value["phases"][1]["verdict_routes"][1]["transition"] = json!("block");
    assert_failure(
        parse_definition(value).validate(),
        "WP6-PHASE-ROUTE-REVISIT-TRANSITION",
        "pipeline_definition.phases[1].verdict_routes[1].transition",
    );
    for (required, dispositions, suffix) in [
        (true, json!([]), "DISPOSITION-REQUIRED"),
        (false, json!(["X"]), "DISPOSITION-MEMBERSHIP"),
    ] {
        let mut value = route_definition();
        value["phases"][1]["allowed_dispositions"] = json!(["D"]);
        value["phases"][1]["disposition_required"] = json!(required);
        value["phases"][1]["verdict_routes"][0]["dispositions"] = json!(["D"]);
        value["phases"][1]["verdict_routes"][1]["dispositions"] = dispositions;
        let path_field = if required {
            "dispositions"
        } else {
            "dispositions[0]"
        };
        assert_failure(
            parse_definition(value).validate(),
            &format!("WP6-PHASE-ROUTE-{suffix}"),
            &format!("pipeline_definition.phases[1].verdict_routes[1].{path_field}"),
        );
    }
    let mut value = route_definition();
    value["version"] = json!("0.7");
    value["phases"][1]["required_dispositions"] = json!(["D"]);
    value["phases"][1]["verdict_routes"][0]["dispositions"] = json!(["D"]);
    assert_failure(
        parse_definition(value).validate(),
        "WP6-PHASE-ROUTE-COMPLETED-DISPOSITION",
        "pipeline_definition.phases[1].verdict_routes[1].dispositions",
    );
    for (transition, outcome, suffix) in [
        ("complete", "blocked", "COMPLETE-OUTCOME"),
        ("block", "completed", "BLOCK-OUTCOME"),
        ("escalate", "waiting_input", "ESCALATE-OUTCOME"),
    ] {
        let mut value = route_definition();
        value["phases"][1]["verdict_routes"][1]["transition"] = json!(transition);
        value["phases"][1]["verdict_routes"][1]["outcome"] = json!(outcome);
        assert_failure(
            parse_definition(value).validate(),
            &format!("WP6-PHASE-ROUTE-{suffix}"),
            "pipeline_definition.phases[1].verdict_routes[1].outcome",
        );
    }
    let mut value = definition();
    value["phases"][0]["verdict_routes"][0]["transition"] = json!("complete");
    assert_failure(
        parse_definition(value).validate(),
        "WP6-PHASE-ROUTE-COMPLETE-ORDINAL",
        "pipeline_definition.phases[0].ordinal",
    );
    for target in ["missing", "second"] {
        let mut value = definition();
        value["phases"][1]["allowed_backward_to"] = json!(["first", target]);
        assert_failure(
            parse_definition(value).validate(),
            "WP6-PHASE-BACKWARD-TARGET",
            "pipeline_definition.phases[1].allowed_backward_to[1]",
        );
    }
    let mut value = definition();
    value["phases"][1]["allowed_backward_to"] = json!(["first"]);
    value["phases"][1]["verdict_routes"][0]["revisit_to"] = json!(["first"]);
    assert!(parse_definition(value).validate().is_ok());
}
