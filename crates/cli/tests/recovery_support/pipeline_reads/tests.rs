use super::*;

fn origin() -> Value {
    json!({"delivery_scope":"snapshot_reference","run":{"id":"00000000-0000-0000-0000-000000000001","revision":3,
        "definition_kind":"slice.full-design-to-execution","definition_version":"fixture","definition_digest":"a".repeat(64),
        "current_phase_id":"second","status":"active"},"definition":{"kind":"slice.full-design-to-execution",
        "version":"fixture","digest":"a".repeat(64)},"counts":{"phases":2},"actions":[]})
}
fn snapshot(origin: &Value) -> Value {
    json!({"run_id":origin["run"]["id"],"definition_digest":origin["run"]["definition_digest"],
        "definition":{"kind":origin["definition"]["kind"],"version":origin["definition"]["version"],
            "digest":origin["definition"]["digest"],"phases":[{"id":"first","ordinal":1},{"id":"second","ordinal":2}]}})
}
fn action(origin: &Value, view: &str) -> Value {
    let mut params = json!({"run_id":origin["run"]["id"],"view":view});
    if view == "details" {
        params["run_revision"] = origin["run"]["revision"].clone();
        params["section"] = json!("all");
    } else {
        params["definition_digest"] = origin["run"]["definition_digest"].clone();
    }
    if view == "phase_contract" {
        params["phase_id"] = origin["run"]["current_phase_id"].clone();
    }
    json!({"kind":"ready_call","tool":"query","arguments":{"route":ROUTE,"params":params}})
}

#[test]
fn snapshot_identity_and_complete_phase_count_are_required() {
    let context = origin();
    let mut value = snapshot(&context);
    assert!(verify_snapshot(&context, &value).is_ok());
    value["definition"]["phases"].as_array_mut().unwrap().pop();
    assert!(verify_snapshot(&context, &value).is_err());
    value = snapshot(&context);
    value["definition"]["version"] = json!("other");
    assert!(verify_snapshot(&context, &value).is_err());
}
#[test]
fn current_phase_matches_second_stored_phase_and_rejects_ambiguity() {
    let context = origin();
    let mut snapshot = snapshot(&context);
    let phase = json!({"run_id":context["run"]["id"],"definition_digest":context["run"]["definition_digest"],
        "phase":snapshot["definition"]["phases"][1]});
    assert!(verify_phase(&context, &snapshot, &phase).is_ok());
    let duplicate = snapshot["definition"]["phases"][1].clone();
    snapshot["definition"]["phases"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert!(verify_phase(&context, &snapshot, &phase).is_err());
}
#[test]
fn actual_destinations_reject_wrong_pins_and_duplicates() {
    let mut raw = origin();
    raw["actions"] = json!([action(&raw, "snapshot")]);
    assert!(destination(&raw, &raw["run"], "snapshot").is_ok());
    raw["actions"][0]["arguments"]["params"]["definition_digest"] = json!("b".repeat(64));
    assert!(destination(&raw, &raw["run"], "snapshot").is_err());
    let expected = action(&raw, "snapshot");
    raw["actions"] = json!([expected.clone(), expected]);
    assert!(destination(&raw, &raw["run"], "snapshot").is_err());
}
#[test]
fn details_all_destination_remains_exact_with_capacity_inputs_destination() {
    let mut raw = origin();
    let all = action(&raw, "details");
    let mut inputs = all.clone();
    inputs["arguments"]["params"]["section"] = json!("inputs");
    raw["actions"] = json!([all.clone(), inputs.clone()]);
    assert_eq!(destination(&raw, &raw["run"], "details").unwrap(), all);

    raw["actions"] = json!([all.clone(), inputs.clone(), all.clone()]);
    assert!(destination(&raw, &raw["run"], "details").is_err());
    let mut wrong_revision = all;
    wrong_revision["arguments"]["params"]["run_revision"] = json!(4);
    raw["actions"] = json!([wrong_revision, inputs.clone()]);
    assert!(destination(&raw, &raw["run"], "details").is_err());
    raw["actions"] = json!([inputs]);
    assert!(destination(&raw, &raw["run"], "details").is_err());
}
#[test]
fn details_cannot_replace_origin_identity() {
    let context = origin();
    let mut details = json!({"run_id":context["run"]["id"],"run_revision":3,"section":"all","data":{"inputs":[]}});
    assert!(verify_details(&context, &details).is_ok());
    details["data"]["definition"] = json!({});
    assert!(verify_details(&context, &details).is_err());
}
#[test]
fn compact_origin_requires_one_actual_context() {
    let context = origin();
    assert_eq!(compact_context(&context).unwrap(), context);
    let raw = json!({"created":context.clone(),"context":context,"actions":[]});
    assert!(compact_context(&raw).is_err());
}
#[test]
fn continuation_preserves_exact_query_with_only_false_refresh_default() {
    let context = origin();
    let initial = action(&context, "snapshot")["arguments"]["params"].clone();
    assert!(bytes::test_pins(&initial).is_ok());
    let mut params = initial.clone();
    params["offset_bytes"] = json!(4);
    params["limit_bytes"] = json!(4096);
    params["representation_digest"] = json!("f".repeat(64));
    params["refresh"] = json!(false);
    let mut page = json!({"representation_digest":"f".repeat(64),"recommended_action":0,
        "actions":[{"kind":"ready_call","tool":"query","arguments":{"route":ROUTE,"params":params}}]});
    assert!(bytes::test_continuation(&page, &initial, 4).is_ok());
    page["actions"][0]["arguments"]["params"]["refresh"] = json!(true);
    assert!(bytes::test_continuation(&page, &initial, 4).is_err());
    page["actions"][0]["arguments"]["params"]["refresh"] = json!(false);
    page["actions"][0]["arguments"]["params"]["phase_id"] = json!("invented");
    assert!(bytes::test_continuation(&page, &initial, 4).is_err());
}

#[test]
fn provenance_keeps_actual_advertised_and_explicit_queries_distinct() {
    let raw = origin();
    let actual = action(&raw, "snapshot");
    let before = raw.clone();
    let terminal = json!({"actions":[],"recommended_action":null});
    let advertised =
        bytes::test_provenance(Some(&actual), &actual["arguments"], &terminal).unwrap();
    assert_eq!(advertised.initial_action.as_ref(), Some(&actual));
    assert_eq!(advertised.initial_query_arguments, actual["arguments"]);
    assert_eq!(advertised.source["run_id"], raw["run"]["id"]);
    assert_eq!(
        advertised.source["definition_digest"],
        raw["run"]["definition_digest"]
    );
    assert_eq!(advertised.representation_digest, Some("a".repeat(64)));
    assert_eq!(advertised.pages, 2);
    assert_eq!(advertised.maximum_envelope_bytes, 8192);
    assert!(advertised.terminal_actions.is_empty());
    assert!(advertised.terminal_recommended_action.is_null());
    let explicit = bytes::test_provenance(None, &actual["arguments"], &terminal).unwrap();
    assert!(explicit.initial_action.is_none());
    assert_eq!(explicit.initial_query_arguments, actual["arguments"]);
    assert_eq!(raw, before);
    assert!(
        bytes::test_provenance(
            None,
            &actual["arguments"],
            &json!({"actions":[],"recommended_action":0})
        )
        .is_err()
    );
}

#[test]
fn null_phase_requires_completed_or_escalated_and_no_phase_destination() {
    let mut raw = origin();
    raw["run"]["current_phase_id"] = Value::Null;
    for status in ["completed", "escalated"] {
        raw["run"]["status"] = json!(status);
        assert!(current_phase_action(&raw, &raw["run"]).unwrap().is_none());
    }
    for status in ["active", "waiting_input", "blocked", "superseded"] {
        raw["run"]["status"] = json!(status);
        assert!(current_phase_action(&raw, &raw["run"]).is_err());
    }
    raw["run"]["status"] = json!("completed");
    raw["actions"] = json!([action(&raw, "phase_contract")]);
    assert!(current_phase_action(&raw, &raw["run"]).is_err());
    raw["run"]["current_phase_id"] = json!("second");
    assert!(current_phase_action(&raw, &raw["run"]).is_err());
}
