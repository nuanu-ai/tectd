use super::*;

pub(super) async fn verify(client: &mut Mcp, context: &ResolvedPipeline) {
    assert_eq!(
        context.run()["current_phase_id"],
        "slice-design-spec-shaper"
    );
    assert_eq!(context.run()["current_phase_ordinal"], 3);
    let (v, o, t) = successful_route(context);
    let request = completion(context, v, o, t, None, None);
    let phase = context.current_phase().unwrap();
    assert_eq!(phase["resources"].as_array().unwrap().len(), 2);
    let other = context.definition()["phases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|candidate| candidate["ordinal"] == 4)
        .expect("actual next Full phase");
    assert_ne!(other["id"], phase["id"]);
    let mut wrong_phase = request.clone();
    wrong_phase["request_id"] = json!(Uuid::new_v4());
    wrong_phase["phase_id"] = other["id"].clone();
    refuses_with_code_without_persistence(client, context, wrong_phase, "INVALID_OUTPUT").await;
    let resource = &phase["resources"][0];
    let expected_digest = resource["digest"].as_str().unwrap();
    assert_eq!(
        request["output"]["resource_reads"][0]["instruction_id"],
        resource["id"]
    );
    assert_eq!(
        request["output"]["resource_reads"][0]["version"],
        resource["version"]
    );
    assert_eq!(
        request["output"]["resource_reads"][0]["digest"],
        expected_digest
    );
    let mut substituted = request;
    substituted["request_id"] = json!(Uuid::new_v4());
    substituted["output"]["resource_reads"][0]["digest"] = json!("substituted");
    let error =
        refuses_with_code_without_persistence(client, context, substituted, "INVALID_OUTPUT").await;
    assert_eq!(error["error"]["code"], "INVALID_OUTPUT");
    let refusal = &error["error"]["refusal"];
    for (field, expected) in [
        ("rule", "WP6-RESOURCE-READ-01"),
        ("path", "arguments.params.output.resource_reads"),
        ("next_action", "supply_exact_phase_resource_reads"),
        ("required", "exact_phase_resource_reads"),
    ] {
        assert_eq!(refusal[field], expected);
    }
    assert!(
        refusal["expected"]
            .as_str()
            .unwrap()
            .contains(expected_digest),
        "{refusal}"
    );
    assert!(refusal["actual"].as_str().unwrap().contains("substituted"));
}
