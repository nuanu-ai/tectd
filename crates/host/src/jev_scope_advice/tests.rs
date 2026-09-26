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
            format!("choice_{ID}"): {
                "type": "choice",
                "choice": "PREFERRED",
                "confidence": 0.8,
                "probabilities": {"NON_PREFERRED": 0.2, "PREFERRED": 0.8}
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
    assert_eq!(body["questions"][format!("choice_{ID}")]["type"], "choice");
    assert_eq!(body["questions"][format!("score_{ID}")]["type"], "score");
}

#[test]
fn duplicate_keys_are_rejected_at_every_nested_object_level() {
    let choice = format!("choice_{ID}");
    let score = format!("score_{ID}");
    let cases = [
        r#"{"model":"jev-1.13.0","model":"jev-1.13.0","answers":{},"usage":null}"#.to_owned(),
        format!(
            r#"{{"model":"jev-1.13.0","answers":{{"{choice}":{{"type":"choice","type":"choice"}}}},"usage":null}}"#
        ),
        format!(
            r#"{{"model":"jev-1.13.0","answers":{{"{choice}":{{"type":"choice","choice":"PREFERRED","confidence":0.8,"probabilities":{{"PREFERRED":0.8,"PREFERRED":0.8,"NON_PREFERRED":0.2}}}},"{score}":{{}}}},"usage":null}}"#
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
    let choice = format!("choice_{ID}");
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
        ((choice.as_str(), "choice"), json!("MAYBE")),
        ((choice.as_str(), "confidence"), json!(1.1)),
        ((choice.as_str(), "probabilities"), json!({"PREFERRED":1.0})),
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
