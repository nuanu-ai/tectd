#[test]
fn typed_abstain_reasons_cover_policy_boundaries() {
    let p = prepared(2);
    let mut value = response(&p);
    value["answers"][CHOICE_ID]["choice"] = json!(ABSTAIN);
    value["answers"][CHOICE_ID]["probabilities"][ABSTAIN] = json!(0.8);
    value["answers"][CHOICE_ID]["probabilities"][&p.eligible_ids[0]] = json!(0.2);
    assert_eq!(
        parse(&value, &p).unwrap().abstain_reason,
        Some(PipelineNativeAbstainReason::ProviderAbstained)
    );
    let mut value = response(&p);
    value["answers"][CHOICE_ID]["probabilities"][&p.eligible_ids[0]] = json!(0.6);
    value["answers"][CHOICE_ID]["probabilities"][ABSTAIN] = json!(0.4);
    assert_eq!(
        parse(&value, &p).unwrap().abstain_reason,
        Some(PipelineNativeAbstainReason::LowChoiceProbability)
    );
    let mut value = response(&p);
    value["answers"][CHOICE_ID]["confidence"] = json!(0.69);
    assert_eq!(
        parse(&value, &p).unwrap().abstain_reason,
        Some(PipelineNativeAbstainReason::LowChoiceConfidence)
    );
    let mut value = response(&p);
    value["answers"]["score_v1_0"] = score_answer(7);
    assert_eq!(
        parse(&value, &p).unwrap().abstain_reason,
        Some(PipelineNativeAbstainReason::ChoiceScoreDisagreement)
    );
    let mut value = response(&p);
    value["answers"]["score_v1_1"]["score"] = json!(8.95);
    value["answers"]["score_v1_1"]["probabilities"]["8"] = json!(0.05);
    value["answers"]["score_v1_1"]["probabilities"]["9"] = json!(0.95);
    assert_eq!(
        parse(&value, &p).unwrap().abstain_reason,
        Some(PipelineNativeAbstainReason::InsufficientScoreSeparation)
    );
}

#[test]
fn rejects_malformed_unknown_missing_duplicate_ids_and_incoherent_answers() {
    let p = prepared(2);
    assert_eq!(
        parse_native_response(b"{", &p, MAX_RESPONSE_BYTES),
        Err(Error::InvalidArguments)
    );
    let mut duplicate = p.clone();
    duplicate.eligible_ids[1] = duplicate.eligible_ids[0].clone();
    assert_eq!(
        parse(&response(&p), &duplicate),
        Err(Error::InvalidArguments)
    );
    let mut value = response(&p);
    value["answers"][CHOICE_ID]["choice"] = json!("unknown-id");
    assert_eq!(parse(&value, &p), Err(Error::InvalidArguments));
    let mut value = response(&p);
    value["answers"]
        .as_object_mut()
        .unwrap()
        .remove("score_v1_1");
    assert_eq!(parse(&value, &p), Err(Error::InvalidArguments));
    let mut value = response(&p);
    value["answers"]["score_v1_2"] = score_answer(4);
    assert_eq!(parse(&value, &p), Err(Error::InvalidArguments));
    let mut value = response(&p);
    value["answers"]["score_v1_0"]["score"] = json!(8.5);
    assert_eq!(parse(&value, &p), Err(Error::InvalidArguments));
    let mut value = response(&p);
    value["answers"]["score_v1_0"]["legend"]["9"] = json!("altered");
    assert_eq!(parse(&value, &p), Err(Error::InvalidArguments));
    let mut value = response(&p);
    value["model"] = json!("unexpected-model");
    assert_eq!(parse(&value, &p), Err(Error::InvalidArguments));
}
