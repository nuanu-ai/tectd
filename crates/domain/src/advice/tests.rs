use super::*;
use serde_json::json;

/// A point whose content is the accepted answers as they came, and which keeps one
/// answer about one candidate.
struct Verbatim;

impl AdviceShaping for Verbatim {
    fn shape(&self, answers: &BTreeMap<String, AdviceAnswer>) -> AdviceField {
        AdviceField::Advised {
            content: [("answers".to_owned(), json!(answers))]
                .into_iter()
                .collect(),
        }
    }
    fn largest(&self) -> AdviceField {
        AdviceField::Abstained
    }
    fn subject(&self) -> String {
        "this test".into()
    }
    fn kept(&self, answers: &BTreeMap<String, AdviceAnswer>) -> Vec<CandidateAdvice> {
        vec![CandidateAdvice {
            level: SizingLevel::ProgramToScopes,
            candidate_id: Uuid::from_u128(7),
            candidate_revision: 2,
            asks_change: answers["size"].choice == "large",
            answer: json!({"size":answers["size"].choice}),
        }]
    }
}

fn questions() -> AdviceQuestionSet {
    serde_json::from_value(json!({
        "state": {"task": "Add invitations"},
        "questions": {
            "size": {
                "instructions": "Choose the size class of the candidate.",
                "criteria": {"small": "One sitting.", "large": "Several days.", "ABSTAIN": "Not enough to tell."}
            }
        }
    }))
    .unwrap()
}

fn answer(choice: &str, small: f64, large: f64, abstain: f64) -> BTreeMap<String, AdviceAnswer> {
    let probabilities = [
        ("small", small),
        ("large", large),
        (ADVICE_ABSTAIN, abstain),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect();
    [(
        "size".to_owned(),
        AdviceAnswer {
            choice: choice.into(),
            probabilities,
            confidence: Some(0.5),
        },
    )]
    .into_iter()
    .collect()
}

#[test]
fn enabling_requires_a_positive_limit_and_there_is_no_default() {
    let request = |enabled, daily_call_limit, expected_revision| ConfigureAdvice {
        enabled,
        daily_call_limit,
        expected_revision,
    };
    assert!(request(true, Some(1), 0).validate().is_ok());
    assert!(request(false, None, 0).validate().is_ok());
    assert!(request(false, Some(20), 3).validate().is_ok());
    for invalid in [
        request(true, None, 0),
        request(true, Some(0), 0),
        request(false, Some(-1), 0),
        request(false, None, -1),
    ] {
        assert_eq!(invalid.validate(), Err(Error::InvalidArguments));
    }
    assert!(
        serde_json::from_value::<ConfigureAdvice>(
            json!({"enabled":true,"daily_call_limit":5,"expected_revision":0,"force":true})
        )
        .is_err()
    );
}

#[test]
fn reply_field_has_exactly_the_three_documented_shapes() {
    let advised = AdviceField::Advised {
        content: json!({"answers":{"size":{"choice":"small"}}})
            .as_object()
            .unwrap()
            .clone(),
    };
    assert_eq!(
        json!(advised),
        json!({"status":"advised","answers":{"size":{"choice":"small"}}})
    );
    assert_eq!(json!(AdviceField::Abstained), json!({"status":"abstained"}));
    let unavailable = AdviceField::unavailable(
        AdviceUnavailableReason::DailyLimitReached,
        "the daily call limit of 3 is reached",
    );
    assert_eq!(
        json!(unavailable),
        json!({"status":"unavailable","reason":"daily_limit_reached",
            "message":"Jev advice is not available: the daily call limit of 3 is reached"})
    );
    for field in [advised, AdviceField::Abstained, unavailable] {
        assert_eq!(
            serde_json::from_value::<AdviceField>(json!(field)).unwrap(),
            field
        );
    }
    assert_eq!(AdviceField::Abstained.outcome(), AdviceOutcome::Abstained);
    assert_eq!(AdviceField::Abstained.reason(), None);
}

#[test]
fn closed_lists_use_the_documented_names() {
    use AdviceUnavailableReason::*;
    let reasons = [
        ProviderUnconfigured,
        DailyLimitReached,
        Timeout,
        TransportError,
        HttpStatus,
        InvalidResponse,
        RequestTooLarge,
        AdviceTooLarge,
        ModeNotSet,
    ];
    assert_eq!(
        reasons.map(AdviceUnavailableReason::as_str),
        [
            "provider_unconfigured",
            "daily_limit_reached",
            "timeout",
            "transport_error",
            "http_status",
            "invalid_response",
            "request_too_large",
            "advice_too_large",
            "mode_not_set"
        ]
    );
    for reason in reasons {
        assert_eq!(json!(reason), json!(reason.as_str()));
    }
    for outcome in [
        AdviceOutcome::Advised,
        AdviceOutcome::Abstained,
        AdviceOutcome::Unavailable,
    ] {
        assert_eq!(json!(outcome), json!(outcome.as_str()));
    }
    for certainty in [
        AdviceSendCertainty::NotSent,
        AdviceSendCertainty::Sent,
        AdviceSendCertainty::SentUnknown,
    ] {
        assert_eq!(json!(certainty), json!(certainty.as_str()));
    }
}

#[test]
fn question_sets_need_an_object_state_and_an_abstain_criterion() {
    assert!(questions().validate().is_ok());
    let mut text_state = questions();
    text_state.state = json!("plain text");
    let mut empty = questions();
    empty.questions.clear();
    let mut no_abstain = questions();
    no_abstain
        .questions
        .get_mut("size")
        .unwrap()
        .criteria
        .remove(ADVICE_ABSTAIN);
    let mut blank = questions();
    blank.questions.get_mut("size").unwrap().instructions = " ".into();
    for invalid in [text_state, empty, no_abstain, blank] {
        assert_eq!(invalid.validate(), Err(Error::InvalidArguments));
    }
}

#[test]
fn answers_are_accepted_only_under_every_rule() {
    let questions = questions();
    assert_eq!(questions.accept(&answer("small", 0.7, 0.2, 0.1)), Ok(()));
    assert_eq!(questions.accept(&answer("small", 0.4, 0.4, 0.2)), Ok(()));
    // Jev rounds each probability to two decimals: three criteria may miss 1 by 0.015.
    assert_eq!(questions.accept(&answer("small", 0.7, 0.2, 0.09)), Ok(()));
    assert_eq!(questions.accept(&answer("small", 0.7, 0.2, 0.11)), Ok(()));
    // And its choice may read a hundredth below another criterion: the live answer
    // that chose 0.43 beside 0.44.
    assert_eq!(questions.accept(&answer("small", 0.43, 0.44, 0.13)), Ok(()));
    assert!(
        questions
            .accept(&BTreeMap::new())
            .unwrap_err()
            .contains("no answer")
    );
    for (answers, rule) in [
        (answer("small", 0.7, 0.2, 0.2), "do not sum to 1"),
        (answer("small", 0.7, 0.2, 0.08), "do not sum to 1"),
        (answer("small", 0.7, 0.2, 0.12), "do not sum to 1"),
        (answer("large", 0.7, 0.2, 0.1), "not the most probable"),
        (answer("small", 0.42, 0.44, 0.14), "not the most probable"),
        (answer("medium", 0.7, 0.2, 0.1), "was not asked"),
        (answer("small", 1.5, -0.6, 0.1), "outside 0 to 1"),
        (answer("small", f64::NAN, 0.2, 0.1), "outside 0 to 1"),
    ] {
        let error = questions.accept(&answers).unwrap_err();
        assert!(error.contains(rule), "{error}");
    }
    let mut unknown = answer("small", 0.6, 0.2, 0.1);
    unknown
        .get_mut("size")
        .unwrap()
        .probabilities
        .insert("huge".into(), 0.1);
    let mut missing = answer("small", 0.8, 0.2, 0.0);
    missing
        .get_mut("size")
        .unwrap()
        .probabilities
        .remove(ADVICE_ABSTAIN);
    for answers in [unknown, missing] {
        assert!(
            questions
                .accept(&answers)
                .unwrap_err()
                .contains("exactly the criteria asked")
        );
    }
    let mut confidence = answer("small", 0.7, 0.2, 0.1);
    confidence.get_mut("size").unwrap().confidence = Some(1.5);
    assert!(
        questions
            .accept(&confidence)
            .unwrap_err()
            .contains("confidence")
    );
    confidence.get_mut("size").unwrap().confidence = None;
    assert_eq!(questions.accept(&confidence), Ok(()));
}

#[test]
fn provider_results_become_the_row_completion() {
    let exchange = AdviceExchange {
        model: Some("jev-latest".into()),
        input_tokens: Some(120),
        ..AdviceExchange::default()
    };
    let advised = AdviceCompletion::from_provider(
        Ok(AdviceReply {
            answers: answer("small", 0.7, 0.2, 0.1),
            exchange: exchange.clone(),
        }),
        &Verbatim,
    );
    assert_eq!(advised.send_certainty, AdviceSendCertainty::Sent);
    assert_eq!(advised.exchange, exchange);
    assert_eq!(
        json!(advised.advice),
        json!({"status":"advised","answers":{"size":{"choice":"small",
            "probabilities":{"ABSTAIN":0.1,"large":0.2,"small":0.7},"confidence":0.5}}})
    );
    // An advised completion carries what the point keeps of the answers.
    assert_eq!(
        advised.kept,
        [CandidateAdvice {
            level: SizingLevel::ProgramToScopes,
            candidate_id: Uuid::from_u128(7),
            candidate_revision: 2,
            asks_change: false,
            answer: json!({"size":"small"}),
        }]
    );
    // Answers that all abstain never reach the point's shaping step, and nothing is
    // kept of them.
    let abstained = AdviceCompletion::from_provider(
        Ok(AdviceReply {
            answers: answer(ADVICE_ABSTAIN, 0.1, 0.2, 0.7),
            exchange: AdviceExchange::default(),
        }),
        &Verbatim,
    );
    assert_eq!(abstained.advice, AdviceField::Abstained);
    assert!(abstained.kept.is_empty());
    let failed = AdviceCompletion::from_provider(
        Err(AdviceFailure {
            reason: AdviceUnavailableReason::Timeout,
            send_certainty: AdviceSendCertainty::SentUnknown,
            detail: "Jev did not answer within 2000 ms".into(),
            exchange: AdviceExchange::default(),
        }),
        &Verbatim,
    );
    assert_eq!(failed.send_certainty, AdviceSendCertainty::SentUnknown);
    assert_eq!(
        failed.advice,
        AdviceField::Unavailable {
            reason: AdviceUnavailableReason::Timeout,
            message: "Jev advice is not available: Jev did not answer within 2000 ms".into(),
        }
    );
    assert_eq!(failed.exchange.input_tokens, None);
    assert!(failed.kept.is_empty());
    let skipped = AdviceFailure::not_called(AdviceUnavailableReason::ProviderUnconfigured, "x");
    assert_eq!(skipped.send_certainty, AdviceSendCertainty::NotSent);
}

#[test]
fn journal_inputs_and_filters_are_bounded() {
    assert!(validate_advice_point("sizing").is_ok());
    assert!(validate_advice_route("scope.candidates.save").is_ok());
    for invalid in [
        "",
        "Sizing",
        "sizing point",
        "sizing.point",
        &"a".repeat(65),
    ] {
        assert_eq!(validate_advice_point(invalid), Err(Error::InvalidArguments));
    }
    assert_eq!(
        validate_advice_route("scope/save"),
        Err(Error::InvalidArguments)
    );
    assert!(AdviceWorkRefs::default().validate().is_ok());
    for invalid in [
        AdviceWorkRefs {
            request_id: Some(Uuid::nil()),
            ..AdviceWorkRefs::default()
        },
        AdviceWorkRefs {
            phase: Some(" ".into()),
            ..AdviceWorkRefs::default()
        },
    ] {
        assert_eq!(invalid.validate(), Err(Error::InvalidArguments));
    }
    let query = |value| serde_json::from_value::<AdviceAuditQuery>(value).unwrap();
    assert!(query(json!({"limit":0,"bodies":false})).validate().is_ok());
    assert!(
        query(
            json!({"limit":100,"bodies":true,"point":"sizing","outcome":"unavailable",
            "since":"2026-10-12T00:00:00Z","until":"2026-10-13T00:00:00+02:00"})
        )
        .validate()
        .is_ok()
    );
    for invalid in [
        json!({"limit":101,"bodies":true}),
        json!({"limit":1,"bodies":true,"point":"Bad Point"}),
        json!({"limit":1,"bodies":true,"since":"yesterday"}),
        json!({"limit":1,"bodies":true,
            "since":"2026-10-13T00:00:00Z","until":"2026-10-12T00:00:00Z"}),
        json!({"limit":1,"bodies":true,"after":Uuid::nil()}),
    ] {
        assert_eq!(query(invalid).validate(), Err(Error::InvalidArguments));
    }
    // The bodies switch is stated by the caller: it has no default.
    for undecodable in [
        json!({"limit":1}),
        json!({"limit":1,"bodies":"no"}),
        json!({"limit":1,"bodies":true,"sql":"x"}),
    ] {
        assert!(serde_json::from_value::<AdviceAuditQuery>(undecodable).is_err());
    }
}

#[test]
fn counters_split_calls_skips_outcomes_and_reasons() {
    let mut counters = AdviceCounters::default();
    counters.add(true, Some(AdviceOutcome::Advised), None, 4);
    counters.add(true, Some(AdviceOutcome::Abstained), None, 1);
    counters.add(true, None, None, 2);
    counters.add(
        true,
        Some(AdviceOutcome::Unavailable),
        Some(AdviceUnavailableReason::Timeout),
        3,
    );
    counters.add(
        false,
        Some(AdviceOutcome::Unavailable),
        Some(AdviceUnavailableReason::DailyLimitReached),
        5,
    );
    assert_eq!(
        json!(counters),
        json!({"called":10,"not_called":5,"advised":4,"abstained":1,"unavailable":8,
            "pending":2,"reasons":{"daily_limit_reached":5,"timeout":3}})
    );
}
