use super::*;

#[test]
fn phase_and_route_multiple_violations_keep_original_first_failure_order() {
    let mut value = definition();
    value["phases"][1]["ordinal"] = json!(1);
    value["phases"][1]["id"] = json!("first");
    assert_failure(
        parse_definition(value).validate(),
        "WP6-PHASE-ORDINAL",
        "pipeline_definition.phases[1].ordinal",
    );
    let mut value = definition();
    value["phases"][1]["required_fields"] = json!([" "]);
    value["phases"][1]["allowed_dispositions"] = json!(["D", "D"]);
    assert_failure(
        parse_definition(value).validate(),
        "WP6-PHASE-ALLOWED-DISPOSITION-DUPLICATE",
        "pipeline_definition.phases[1].allowed_dispositions[1]",
    );
    let mut value = route_definition();
    value["phases"][1]["verdict_routes"][1]["dispositions"] = json!([" ", " "]);
    value["phases"][1]["verdict_routes"][1]["revisit_to"] = json!(["missing"]);
    assert_failure(
        parse_definition(value).validate(),
        "WP6-PHASE-ROUTE-DISPOSITION-DUPLICATE",
        "pipeline_definition.phases[1].verdict_routes[1].dispositions[1]",
    );
    let mut value = route_definition();
    value["phases"][1]["allowed_dispositions"] = json!(["D"]);
    value["phases"][1]["verdict_routes"][1]["dispositions"] = json!(["X", " "]);
    assert_failure(
        parse_definition(value).validate(),
        "WP6-PHASE-ROUTE-DISPOSITION-MEMBERSHIP",
        "pipeline_definition.phases[1].verdict_routes[1].dispositions[0]",
    );
    let mut value = amendment();
    value["predecessor"]["artifact_name"] = json!("");
    value["successor"]["artifact"]["body"] = json!("");
    assert_failure(
        validate_source_amendment(&serde_json::from_value(value).unwrap()),
        "WP6-SOURCE-AMENDMENT-PREDECESSOR-ARTIFACT-NAME-BLANK",
        "arguments.params.source_amendment.predecessor.artifact_name",
    );
}
