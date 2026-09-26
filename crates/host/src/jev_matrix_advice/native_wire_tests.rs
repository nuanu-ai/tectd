use super::*;
use serde_json::{Value, json};
use tect_domain::{EngineeringCandidate, EngineeringChoiceSet, MATRIX_CHOICE_SET_SCHEMA};

#[test]
fn duplicate_json_counters_and_answers_are_rejected() {
    let valid = serde_json::to_string(&response()).unwrap();
    for raw in [
        valid.replace(
            "\"input_tokens\":20",
            "\"input_tokens\":999999,\"input_tokens\":0",
        ),
        valid.replace("\"usage\":", "\"usage\":{},\"usage\":"),
        valid.replace(
            "\"choice\":\"C0\"",
            "\"choice\":\"ABSTAIN\",\"choice\":\"C0\"",
        ),
    ] {
        assert!(parse_native_response(raw.as_bytes(), &prepared(), 16_384).is_err());
    }
}

fn prepared() -> PreparedNativeMatrixRequest {
    PreparedNativeMatrixRequest {
        body: Vec::new(),
        model: "jev-1.13.0".into(),
        binding: MatrixRankingBinding {
            task_id: "task".into(),
            task_revision: "1".into(),
            input_digest: "a".repeat(64),
            choice_set_id: "set".into(),
            choice_set_version: 1,
            choice_set_digest: "b".repeat(64),
            evaluation_digest: "c".repeat(64),
            verification_digest: Some("d".repeat(64)),
        },
        eligibility: MatrixAdviceEligibility::EligibleForAdvice {
            candidate_ids: vec!["id-a".into(), "id-b".into()],
        },
        token_to_candidate_id: BTreeMap::from([
            ("C0".into(), "id-a".into()),
            ("C1".into(), "id-b".into()),
        ]),
    }
}

fn score_answer(level: usize) -> Value {
    let probabilities = (0..10)
        .map(|index| {
            (
                index.to_string(),
                json!(if index == level { 1.0 } else { 0.0 }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let legend = SCORE_LEVELS
        .iter()
        .enumerate()
        .map(|(index, description)| (index.to_string(), json!(description)))
        .collect::<serde_json::Map<_, _>>();
    json!({
        "type": "score",
        "score": level,
        "legend": legend,
        "probabilities": probabilities,
        "confidence": 0.9,
    })
}

fn response() -> Value {
    json!({
        "model": "jev-1.13.0",
        "answers": {
            "score_v1_C0": score_answer(8),
            "score_v1_C1": score_answer(4),
            "choice_v1": {
                "type": "choice",
                "choice": "C0",
                "probabilities": {"C0": 0.8, "C1": 0.1, "ABSTAIN": 0.1},
                "confidence": 0.75,
            }
        },
        "usage": {"input_tokens": 20, "output_tokens": 30},
    })
}

fn parse(value: &Value) -> Result<ParsedNativeMatrixResponse> {
    parse_native_response(&serde_json::to_vec(value).unwrap(), &prepared(), 16_384)
}

#[test]
fn parses_exact_native_answer_and_opaque_token_mapping() {
    let parsed = parse(&response()).unwrap();
    assert_eq!(parsed.signals.candidate_scores.len(), 2);
    assert_eq!(parsed.signals.candidate_scores[0].candidate_id, "id-a");
    assert_eq!(parsed.signals.candidate_scores[0].score, 8.0);
    assert_eq!(parsed.signals.candidate_scores[1].candidate_id, "id-b");
    assert_eq!(
        parsed.signals.choice,
        NativeMatrixChoice::Candidate("id-a".into())
    );
    assert_eq!(parsed.signals.choice_selected_answer_probability, 0.8);
    assert_eq!((parsed.input_tokens, parsed.output_tokens), (20, 30));
}

#[test]
fn fractional_score_is_weighted_mean_without_quantization() {
    let mut value = response();
    value["answers"]["score_v1_C0"]["score"] = json!(7.25);
    value["answers"]["score_v1_C0"]["probabilities"]["7"] = json!(0.75);
    value["answers"]["score_v1_C0"]["probabilities"]["8"] = json!(0.25);
    let parsed = parse(&value).unwrap();
    assert_eq!(parsed.signals.candidate_scores[0].score, 7.25);
}

#[test]
fn explicit_abstain_is_preserved_as_signal() {
    let mut value = response();
    value["answers"]["choice_v1"]["choice"] = json!("ABSTAIN");
    value["answers"]["choice_v1"]["probabilities"] =
        json!({"C0": 0.05, "C1": 0.05, "ABSTAIN": 0.9});
    assert_eq!(
        parse(&value).unwrap().signals.choice,
        NativeMatrixChoice::Abstain
    );
}

#[test]
fn rejects_missing_extra_and_wrong_typed_answers() {
    let mut missing = response();
    missing["answers"]
        .as_object_mut()
        .unwrap()
        .remove("score_v1_C1");
    assert_eq!(parse(&missing), Err(Error::InvalidArguments));
    let mut extra = response();
    extra["answers"]["score_v1_C2"] = score_answer(3);
    assert_eq!(parse(&extra), Err(Error::InvalidArguments));
    let mut wrong_type = response();
    wrong_type["answers"]["score_v1_C0"]["type"] = json!("choice");
    assert_eq!(parse(&wrong_type), Err(Error::InvalidArguments));
    let mut extra_field = response();
    extra_field["answers"]["choice_v1"]["other"] = json!(true);
    assert_eq!(parse(&extra_field), Err(Error::InvalidArguments));
}

#[test]
fn rejects_incoherent_distributions_legends_and_models() {
    let mut value = response();
    value["answers"]["score_v1_C0"]["probabilities"]["8"] = json!(0.9);
    assert_eq!(parse(&value), Err(Error::InvalidArguments));
    let mut value = response();
    value["answers"]["score_v1_C0"]["score"] = json!(7.9);
    assert_eq!(parse(&value), Err(Error::InvalidArguments));
    let mut value = response();
    value["answers"]["score_v1_C0"]["legend"]["9"] = json!("changed");
    assert_eq!(parse(&value), Err(Error::InvalidArguments));
    let mut value = response();
    value["answers"]["choice_v1"]["probabilities"]["other"] = json!(0.0);
    assert_eq!(parse(&value), Err(Error::InvalidArguments));
    let mut value = response();
    value["model"] = json!("unexpected-model");
    assert_eq!(parse(&value), Err(Error::InvalidArguments));
    let mut value = response();
    value["answers"]["choice_v1"]["probabilities"] = json!({"C0": 0.1, "C1": 0.8, "ABSTAIN": 0.1});
    assert_eq!(parse(&value), Err(Error::InvalidArguments));
}

#[test]
fn rejects_malformed_bytes_missing_usage_and_oversized_response() {
    let prepared = prepared();
    assert_eq!(
        parse_native_response(b"{", &prepared, 16_384),
        Err(Error::InvalidArguments)
    );
    let mut missing_usage = response();
    missing_usage.as_object_mut().unwrap().remove("usage");
    assert_eq!(parse(&missing_usage), Err(Error::InvalidArguments));
    let bytes = serde_json::to_vec(&response()).unwrap();
    assert_eq!(
        parse_native_response(&bytes, &prepared, bytes.len() - 1),
        Err(Error::RequestTooLarge)
    );
}

#[test]
fn canonical_candidate_order_and_assumptions_are_input_order_independent() {
    let candidate = |id: &str, assumptions: Vec<&str>| EngineeringCandidate {
        candidate_id: id.into(),
        title: format!("Candidate {id}"),
        approach: "approach".into(),
        assumption_fact_ids: assumptions.into_iter().map(str::to_owned).collect(),
    };
    let first = EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: "set".into(),
        version: 1,
        task_id: "task".into(),
        task_revision: "1".into(),
        decision_question: "question".into(),
        candidates: vec![
            candidate("id-b", vec!["z", "a"]),
            candidate("id-a", vec!["y", "x"]),
        ],
    };
    let mut second = first.clone();
    second.candidates.reverse();
    for candidate in &mut second.candidates {
        candidate.assumption_fact_ids.reverse();
    }
    assert_eq!(canonical_choice_set(&first), canonical_choice_set(&second));
    assert_eq!(
        canonical_choice_set(&first).candidates[0].candidate_id,
        "id-a"
    );
}
