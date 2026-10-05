use super::*;

#[test]
fn begin_fields_version_modes_inquiry_checkpoint_and_priority_are_typed() {
    let definition = parse_definition(definition());
    let valid: BeginPipelineRun = serde_json::from_value(begin()).unwrap();
    assert!(valid.validate(&definition).is_ok());
    for (field, changed, suffix) in [
        ("request_id", json!(Uuid::nil()), "REQUEST-ID"),
        ("scope_id", json!(Uuid::nil()), "SCOPE-ID"),
        ("slice_id", json!(Uuid::nil()), "SLICE-ID"),
        ("slice_revision", json!(0), "SLICE-REVISION"),
        ("qualification_reason", json!(" "), "QUALIFICATION"),
        ("definition_version", json!(" "), "DEFINITION-VERSION-BLANK"),
        (
            "definition_version",
            json!("x".repeat(129)),
            "DEFINITION-VERSION-LIMIT",
        ),
        (
            "definition_version",
            json!("different"),
            "DEFINITION-VERSION-PIN",
        ),
        ("delivery_mode", json!("whole"), "DELIVERY-MODE"),
        ("inquiry", inquiry(), "INQUIRY"),
        (
            "source_checkpoint",
            json!({"checkpoint_id":Uuid::new_v4(),"digest":"a".repeat(64)}),
            "SOURCE-CHECKPOINT",
        ),
    ] {
        let mut value = begin();
        value[field] = changed;
        let request: BeginPipelineRun = serde_json::from_value(value).unwrap();
        assert_failure(
            request.validate(&definition),
            &format!("WP6-BEGIN-{suffix}"),
            &format!("arguments.params.{field}"),
        );
    }
    let mut value = begin();
    value["request_id"] = json!(Uuid::nil());
    value["inquiry"] = inquiry();
    value["definition_version"] = json!("wrong");
    assert_failure(
        serde_json::from_value::<BeginPipelineRun>(value)
            .unwrap()
            .validate(&definition),
        "WP6-BEGIN-REQUEST-ID",
        "arguments.params.request_id",
    );
    let mut research = definition.clone();
    research.kind = PipelineKind::Research;
    assert_failure(
        valid.validate(&research),
        "WP6-BEGIN-INQUIRY",
        "arguments.params.inquiry",
    );
    let mut value = begin();
    value["inquiry"] = inquiry();
    let req: BeginPipelineRun = serde_json::from_value(value.clone()).unwrap();
    assert!(req.validate(&research).is_ok());
    value["source_checkpoint"] = json!({"checkpoint_id":Uuid::nil(),"digest":"bad"});
    assert_failure(
        serde_json::from_value::<BeginPipelineRun>(value)
            .unwrap()
            .validate(&research),
        "WP6-BEGIN-SOURCE-CHECKPOINT",
        "arguments.params.source_checkpoint",
    );
    let mut decision = definition.clone();
    decision.kind = PipelineKind::DeepBrainstorming;
    assert_failure(
        req.validate(&decision),
        "WP6-BEGIN-INQUIRY",
        "arguments.params.inquiry",
    );
    let mut value = begin();
    value["inquiry"] = inquiry();
    value["inquiry"]["completion"] = json!({"kind":"decision","requested_outcome":"decision"});
    assert!(
        serde_json::from_value::<BeginPipelineRun>(value)
            .unwrap()
            .validate(&decision)
            .is_ok()
    );
}
#[test]
fn record_input_and_delivery_escalation_each_request_field_is_typed() {
    assert!(
        serde_json::from_value::<RecordPipelineInput>(input())
            .unwrap()
            .validate()
            .is_ok()
    );
    for (field, changed, suffix) in [
        ("request_id", json!(Uuid::nil()), "REQUEST-ID"),
        ("run_id", json!(Uuid::nil()), "RUN-ID"),
        ("run_revision", json!(0), "REVISION"),
        ("phase_id", json!(" "), "PHASE-ID"),
        ("input", json!(" "), "BODY"),
        ("input", json!("界".repeat(21846)), "SIZE"),
    ] {
        let mut value = input();
        value[field] = changed;
        let request: RecordPipelineInput = serde_json::from_value(value).unwrap();
        let error = request.validate().unwrap_err();
        assert!(!serde_json::to_string(&error).unwrap().contains("界"));
        assert_failure(
            Err(error),
            &format!("WP6-INPUT-{suffix}"),
            &format!("arguments.params.{field}"),
        );
    }
    let valid = json!({"request_id":Uuid::new_v4(),"run_id":Uuid::new_v4(),"run_revision":1,"phase_id":"phase","reason":"reason"});
    assert!(
        serde_json::from_value::<EscalatePipelineDelivery>(valid.clone())
            .unwrap()
            .validate()
            .is_ok()
    );
    for (field, changed, suffix) in [
        ("request_id", json!(Uuid::nil()), "REQUEST-ID"),
        ("run_id", json!(Uuid::nil()), "RUN-ID"),
        ("run_revision", json!(0), "REVISION"),
        ("phase_id", json!(" "), "PHASE-ID"),
        ("reason", json!(" "), "REASON"),
    ] {
        let mut value = valid.clone();
        value[field] = changed;
        assert_failure(
            serde_json::from_value::<EscalatePipelineDelivery>(value)
                .unwrap()
                .validate(),
            &format!("WP6-DELIVERY-ESCALATE-{suffix}"),
            &format!("arguments.params.{field}"),
        );
    }
    let mut value = input();
    value["input"] = json!("x".repeat(MAX_PIPELINE_INPUT_BYTES));
    assert!(
        serde_json::from_value::<RecordPipelineInput>(value)
            .unwrap()
            .validate()
            .is_ok()
    );
}
