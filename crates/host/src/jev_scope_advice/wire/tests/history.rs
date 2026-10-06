use super::super::{parse_response_v2, serialize_request_v2};
use super::*;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
struct OriginalV2Body<'a> {
    model: &'a str,
    state: OriginalV2State<'a>,
    questions: BTreeMap<String, OriginalV2Question>,
}
#[derive(Serialize)]
struct OriginalV2State<'a> {
    request: &'a ScopeAdviceRequest,
    emitted: &'a [tect_domain::ScopeDecompositionAlternative],
}
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OriginalV2Question {
    Choice {
        instructions: &'static str,
        criteria: BTreeMap<&'static str, &'static str>,
    },
    Score {
        instructions: &'static str,
        criteria: [&'static str; 4],
    },
}

#[test]
fn legacy_v2_request_bytes_and_none_answers_are_preserved() {
    let request = request();
    let mut questions = BTreeMap::new();
    questions.insert(format!("choice_{ID}"), OriginalV2Question::Choice {
        instructions: "Choose whether this eligible alternative is preferred using its matching ID and emitted material in state.",
        criteria: BTreeMap::from([("PREFERRED", "preferred for the supplied scope"), ("NON_PREFERRED", "not preferred for the supplied scope")]),
    });
    questions.insert(format!("score_{ID}"), OriginalV2Question::Score {
        instructions: "Score this eligible alternative using its matching ID and emitted material in state.",
        criteria: ["conflict", "weak_fit", "fit", "strong_fit"],
    });
    let original = serde_json::to_vec(&OriginalV2Body {
        model: "jev-1.13.0",
        state: OriginalV2State {
            request: &request,
            emitted: &[],
        },
        questions,
    })
    .unwrap();
    let bytes = serialize_request_v2("jev-1.13.0", &request, &[]).unwrap();
    assert_eq!(bytes, original);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(body["state"].get("candidate_tokens").is_none());
    let mut response = valid_response();
    response["answers"]
        .as_object_mut()
        .unwrap()
        .remove("choice_v3");
    response["answers"][format!("choice_{ID}")] = json!({"type":"choice", "choice":"PREFERRED", "confidence":0.8, "probabilities":{"NON_PREFERRED":0.2,"PREFERRED":0.8}});
    let raw = serde_json::to_vec(&response).unwrap();
    let parsed = parse_response_v2(&raw, "jev-1.13.0", &request)
        .unwrap()
        .answers;
    assert_eq!(parsed.comparative_disposition, None);
    assert_eq!(
        parsed.answers[0].choice,
        tect_domain::ScopeAdviceChoice::Preferred
    );
    assert!(
        serde_json::to_value(&parsed)
            .unwrap()
            .get("comparative_disposition")
            .is_none()
    );
    assert!(parse_response(&raw, "jev-1.13.0", &request).is_err());
}
