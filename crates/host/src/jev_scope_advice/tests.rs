use super::wire::{parse_response, serialize_request};
use super::*;
use serde_json::{Value, json};
use tect_domain::{
    ScopeAdviceAlternative, ScopeAdviceQuestion, ScopeAdviceRequest, ScopeAlternativeId,
    ScopeDecompositionKind,
};
use uuid::Uuid;

mod http;
mod sealed;
mod source_guard;

const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ID_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DISPATCH_ID: Uuid = Uuid::from_u128(7);

fn request() -> ScopeAdviceRequest {
    ScopeAdviceRequest {
        contract: "tect.scope-decomposition-advice/1".into(),
        source_digest: "b".repeat(64),
        manifest_digest: "c".repeat(64),
        eligible_set_digest: "d".repeat(64),
        baseline_id: ScopeAlternativeId(ID.into()),
        alternatives: vec![ScopeAdviceAlternative {
            id: ScopeAlternativeId(ID.into()),
            kind: ScopeDecompositionKind::Cohesive,
            material_digest: "e".repeat(64),
            covered_obligation_ids: vec!["obligation.one".into()],
        }],
        questions: vec![ScopeAdviceQuestion {
            alternative_id: ScopeAlternativeId(ID.into()),
            require_choice: true,
            require_score: true,
        }],
        digest: "f".repeat(64),
    }
}

fn valid_response() -> Value {
    json!({
        "model": "jev-1.13.0",
        "answers": {
            "choice_v3": {
                "type": "choice",
                "choice": "C0",
                "confidence": 0.8,
                "probabilities": {"C0": 0.8, "ABSTAIN": 0.2}
            },
            format!("score_{ID}"): {
                "type": "score",
                "score": 2.4,
                "confidence": 0.7,
                "legend": {"0":"conflict", "1":"weak_fit", "2":"fit", "3":"strong_fit"},
                "probabilities": {"0":0.05, "1":0.1, "2":0.55, "3":0.3}
            }
        },
        "usage": {"input_tokens": 11, "output_tokens": 5}
    })
}

fn two_candidate_request() -> ScopeAdviceRequest {
    let mut request = request();
    let mut second = request.alternatives[0].clone();
    second.id = ScopeAlternativeId(ID_B.into());
    request.alternatives.push(second);
    request.questions.push(ScopeAdviceQuestion {
        alternative_id: ScopeAlternativeId(ID_B.into()),
        require_choice: true,
        require_score: true,
    });
    request
}

fn two_candidate_response() -> Value {
    let mut response = valid_response();
    response["answers"]["choice_v3"]["probabilities"] = json!({"C0":0.7,"C1":0.2,"ABSTAIN":0.1});
    response["answers"][format!("score_{ID_B}")] = json!({
        "type":"score", "score":1.4, "confidence":0.7,
        "legend":{"0":"conflict","1":"weak_fit","2":"fit","3":"strong_fit"},
        "probabilities":{"0":0.1,"1":0.5,"2":0.3,"3":0.1}
    });
    response
}

#[test]
fn one_comparative_choice_binds_tokens_and_normalizes_one_preferred() {
    let request = two_candidate_request();
    let body: Value =
        serde_json::from_slice(&serialize_request("jev-1.13.0", &request, &[]).unwrap()).unwrap();
    assert_eq!(body["questions"].as_object().unwrap().len(), 3);
    assert_eq!(body["state"]["candidate_tokens"]["C0"]["id"], ID);
    assert_eq!(body["state"]["candidate_tokens"]["C1"]["id"], ID_B);
    let bytes = serde_json::to_vec(&two_candidate_response()).unwrap();
    let answers = parse_response(&bytes, "jev-1.13.0", &request)
        .unwrap()
        .answers;
    assert_eq!(
        answers.comparative_disposition,
        Some(tect_domain::ComparativeDisposition::Selected(
            ScopeAlternativeId(ID.into())
        ))
    );
    let answers = answers.answers;
    assert_eq!(
        answers
            .iter()
            .filter(|a| a.choice == tect_domain::ScopeAdviceChoice::Preferred)
            .count(),
        1
    );
    assert_eq!(answers[0].alternative_id.0, ID);
    assert_eq!(
        answers[1].choice,
        tect_domain::ScopeAdviceChoice::NonPreferred
    );
    let mut reordered = request.clone();
    reordered.alternatives.reverse();
    reordered.questions.reverse();
    let reordered_body: Value =
        serde_json::from_slice(&serialize_request("jev-1.13.0", &reordered, &[]).unwrap()).unwrap();
    assert_eq!(
        reordered_body["state"]["candidate_tokens"],
        body["state"]["candidate_tokens"]
    );
    let answers = parse_response(&bytes, "jev-1.13.0", &reordered)
        .unwrap()
        .answers
        .answers;
    assert_eq!(answers[0].alternative_id.0, ID_B);
    assert_eq!(answers[1].choice, tect_domain::ScopeAdviceChoice::Preferred);
}

#[test]
fn comparative_abstain_and_invalid_choice_score_conflicts() {
    let request = two_candidate_request();
    let base = two_candidate_response();
    let mut abstain = base.clone();
    abstain["answers"]["choice_v3"]["choice"] = json!("ABSTAIN");
    abstain["answers"]["choice_v3"]["probabilities"] = json!({"C0":0.2,"C1":0.1,"ABSTAIN":0.7});
    let answers = parse_response(
        &serde_json::to_vec(&abstain).unwrap(),
        "jev-1.13.0",
        &request,
    )
    .unwrap()
    .answers;
    assert_eq!(
        answers.comparative_disposition,
        Some(tect_domain::ComparativeDisposition::Abstain)
    );
    let answers = answers.answers;
    assert!(
        answers
            .iter()
            .all(|a| a.choice == tect_domain::ScopeAdviceChoice::NonPreferred)
    );
    let mut cases = Vec::new();
    let mut value = base.clone();
    value["answers"]["choice_v3"]["choice"] = json!("C2");
    cases.push(value);
    let mut value = base.clone();
    value["answers"]["choice_v3"]["probabilities"] = json!({"C0":0.2,"C1":0.7,"ABSTAIN":0.1});
    cases.push(value);
    let mut value = base.clone();
    value["answers"][format!("score_{ID_B}")]["score"] = json!(2.5);
    cases.push(value);
    let mut value = base.clone();
    value["answers"]
        .as_object_mut()
        .unwrap()
        .remove(&format!("score_{ID_B}"));
    cases.push(value);
    let mut value = base.clone();
    value["answers"]["score_unknown"] = value["answers"][format!("score_{ID_B}")].clone();
    cases.push(value);
    for value in cases {
        assert!(
            parse_response(&serde_json::to_vec(&value).unwrap(), "jev-1.13.0", &request).is_err()
        );
    }
    // An exact tie is accepted only because the provider explicitly selected C0.
    let mut tie = base;
    tie["answers"][format!("score_{ID_B}")]["score"] = json!(2.4);
    tie["answers"]["choice_v3"]["probabilities"] = json!({"C0":0.45,"C1":0.45,"ABSTAIN":0.1});
    assert!(parse_response(&serde_json::to_vec(&tie).unwrap(), "jev-1.13.0", &request).is_ok());
}

#[test]
fn comparative_choice_accepts_selected_c1_with_tiny_raw_score_gap_in_same_band() {
    let request = two_candidate_request();
    let mut response = two_candidate_response();
    response["answers"]["choice_v3"]["choice"] = json!("C1");
    response["answers"]["choice_v3"]["confidence"] = json!(0.75);
    response["answers"]["choice_v3"]["probabilities"] = json!({"C0":0.09,"C1":0.83,"ABSTAIN":0.08});
    response["answers"][format!("score_{ID}")]["score"] = json!(2.52);
    response["answers"][format!("score_{ID_B}")]["score"] = json!(2.51);
    let parsed = parse_response(
        &serde_json::to_vec(&response).unwrap(),
        "jev-1.13.0",
        &request,
    )
    .unwrap()
    .answers;
    assert_eq!(
        parsed.comparative_disposition,
        Some(tect_domain::ComparativeDisposition::Selected(
            ScopeAlternativeId(ID_B.into())
        ))
    );
    assert_eq!(
        parsed.answers[0].score,
        tect_domain::ScopeAdviceScoreBand::StrongFit
    );
    assert_eq!(
        parsed.answers[1].score,
        tect_domain::ScopeAdviceScoreBand::StrongFit
    );
    assert_eq!(
        parsed.answers[0].choice,
        tect_domain::ScopeAdviceChoice::NonPreferred
    );
    assert_eq!(
        parsed.answers[1].choice,
        tect_domain::ScopeAdviceChoice::Preferred
    );
    assert_eq!(parsed.answers[1].choice_confidence.0, 7500);

    response["answers"][format!("score_{ID_B}")]["score"] = json!(2.49);
    assert!(
        parse_response(
            &serde_json::to_vec(&response).unwrap(),
            "jev-1.13.0",
            &request
        )
        .is_err()
    );
}

#[test]
fn request_has_exact_model_state_questions_shape() {
    let request = request();
    let bytes = serialize_request("jev-1.13.0", &request, &[]).unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body.as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        ["model", "questions", "state"]
    );
    assert_eq!(body["model"], "jev-1.13.0");
    assert_eq!(
        body["state"]["request"],
        serde_json::to_value(&request).unwrap()
    );
    assert_eq!(body["state"]["emitted"], json!([]));
    assert_eq!(body["questions"].as_object().unwrap().len(), 2);
    assert_eq!(body["questions"]["choice_v3"]["type"], "choice");
    assert_eq!(body["state"]["candidate_tokens"]["C0"]["id"], ID);
    assert_eq!(body["questions"][format!("score_{ID}")]["type"], "score");
}

#[test]
fn duplicate_keys_are_rejected_at_every_nested_object_level() {
    let choice = "choice_v3".to_owned();
    let score = format!("score_{ID}");
    let cases = [
        r#"{"model":"jev-1.13.0","model":"jev-1.13.0","answers":{},"usage":null}"#.to_owned(),
        format!(
            r#"{{"model":"jev-1.13.0","answers":{{"{choice}":{{"type":"choice","type":"choice"}}}},"usage":null}}"#
        ),
        format!(
            r#"{{"model":"jev-1.13.0","answers":{{"{choice}":{{"type":"choice","choice":"C0","confidence":0.8,"probabilities":{{"C0":0.8,"C0":0.8,"ABSTAIN":0.2}}}},"{score}":{{}}}},"usage":null}}"#
        ),
        format!(
            r#"{{"model":"jev-1.13.0","answers":{{"{choice}":{{}},"{score}":{{"type":"score","score":2,"confidence":1,"probabilities":{{"0":0,"1":0,"2":1,"3":0}},"legend":{{"0":"conflict","0":"conflict","1":"weak_fit","2":"fit","3":"strong_fit"}}}}}},"usage":null}}"#
        ),
    ];
    for body in cases {
        assert!(parse_response(body.as_bytes(), "jev-1.13.0", &request()).is_err());
    }
}

#[test]
fn response_rejects_unknown_missing_and_invalid_native_shapes() {
    let base = valid_response();
    let choice = "choice_v3".to_owned();
    let score = format!("score_{ID}");
    let mut cases = Vec::new();
    let mut value = base.clone();
    value["unknown"] = json!(true);
    cases.push(value);
    let mut value = base.clone();
    value["answers"].as_object_mut().unwrap().remove(&choice);
    cases.push(value);
    let mut value = base.clone();
    value["answers"][&choice]
        .as_object_mut()
        .unwrap()
        .remove("type");
    cases.push(value);
    let mut value = base.clone();
    value["answers"]["unknown"] = value["answers"][&choice].clone();
    cases.push(value);
    let mut value = base.clone();
    value["model"] = json!("jev-other");
    cases.push(value);
    for (pointer, invalid) in [
        ((choice.as_str(), "type"), json!("score")),
        ((choice.as_str(), "choice"), json!("C999")),
        ((choice.as_str(), "confidence"), json!(1.1)),
        ((choice.as_str(), "probabilities"), json!({"C0":1.0})),
        ((score.as_str(), "score"), json!(3.1)),
        ((score.as_str(), "confidence"), json!("high")),
        (
            (score.as_str(), "probabilities"),
            json!({"0":0.2,"1":0.2,"2":0.2,"3":0.2}),
        ),
        (
            (score.as_str(), "legend"),
            json!({"0":"bad","1":"weak_fit","2":"fit","3":"strong_fit"}),
        ),
    ] {
        let mut value = base.clone();
        value["answers"][pointer.0][pointer.1] = invalid;
        cases.push(value);
    }
    for value in cases {
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(parse_response(&bytes, "jev-1.13.0", &request()).is_err());
    }
}

#[test]
fn valid_response_normalizes_only_provider_neutral_answers() {
    let bytes = serde_json::to_vec(&valid_response()).unwrap();
    let parsed = parse_response(&bytes, "jev-1.13.0", &request()).unwrap();
    assert_eq!(parsed.answers.answers.len(), 1);
    let answer = &parsed.answers.answers[0];
    assert_eq!(answer.choice, tect_domain::ScopeAdviceChoice::Preferred);
    assert_eq!(answer.score, tect_domain::ScopeAdviceScoreBand::Fit);
    assert_eq!(answer.choice_confidence.0, 8000);
    assert_eq!(answer.score_confidence.0, 7000);
}
