//! Wire format of one Jev choice call. The codec knows questions and criteria only.
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use tect_domain::{AdviceAnswer, AdviceQuestionSet};

/// `{"model", "state", "questions": {"<name>": {"type": "choice", "instructions", "criteria"}}}`
pub(crate) fn encode_request(model: &str, questions: &AdviceQuestionSet) -> Vec<u8> {
    let asked: Map<String, Value> = questions
        .questions
        .iter()
        .map(|(name, question)| {
            (
                name.clone(),
                json!({"type":"choice","instructions":question.instructions,
                    "criteria":question.criteria}),
            )
        })
        .collect();
    serde_json::to_vec(&json!({"model":model,"state":questions.state,"questions":asked}))
        .expect("JSON value")
}

/// What an answer body yields. Model and usage are read whenever the body is a JSON
/// object; a missing or malformed counter stays unknown.
pub(crate) struct DecodedAnswer {
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    /// The accepted answers, or what was wrong with the body.
    pub answers: std::result::Result<BTreeMap<String, AdviceAnswer>, String>,
}

/// `{"model", "answers": {"<name>": {"type": "choice", "choice", "probabilities",
/// "confidence"}}, "usage": {"input_tokens", "output_tokens"}}`
pub(crate) fn decode_answer(raw: &[u8], questions: &AdviceQuestionSet) -> DecodedAnswer {
    let root = match crate::jev_json::decode_unique_json(raw) {
        Ok(Value::Object(root)) => root,
        Ok(_) => return undecoded("the answer is not a JSON object"),
        Err(_) => return undecoded("the answer is not JSON with unique keys"),
    };
    let counter = |name: &str| {
        root.get("usage")
            .and_then(|usage| usage.get(name))
            .and_then(Value::as_i64)
            .filter(|count| *count >= 0)
    };
    DecodedAnswer {
        model: root.get("model").and_then(Value::as_str).map(str::to_owned),
        input_tokens: counter("input_tokens"),
        output_tokens: counter("output_tokens"),
        answers: answers(&root, questions),
    }
}

fn undecoded(problem: &str) -> DecodedAnswer {
    DecodedAnswer {
        model: None,
        input_tokens: None,
        output_tokens: None,
        answers: Err(problem.into()),
    }
}

fn answers(
    root: &Map<String, Value>,
    questions: &AdviceQuestionSet,
) -> std::result::Result<BTreeMap<String, AdviceAnswer>, String> {
    let given = root
        .get("answers")
        .and_then(Value::as_object)
        .ok_or("the answer has no `answers` object")?;
    let mut answers = BTreeMap::new();
    for name in questions.questions.keys() {
        if let Some(value) = given.get(name) {
            let answer =
                answer(value).ok_or_else(|| format!("the answer to `{name}` is malformed"))?;
            answers.insert(name.clone(), answer);
        }
    }
    questions.accept(&answers)?;
    Ok(answers)
}

fn answer(value: &Value) -> Option<AdviceAnswer> {
    let answer = value.as_object()?;
    if answer.get("type").and_then(Value::as_str) != Some("choice") {
        return None;
    }
    let probabilities = answer
        .get("probabilities")?
        .as_object()?
        .iter()
        .map(|(criterion, probability)| Some((criterion.clone(), probability.as_f64()?)))
        .collect::<Option<BTreeMap<_, _>>>()?;
    let confidence = match answer.get("confidence") {
        Some(confidence) => Some(confidence.as_f64()?),
        None => None,
    };
    Some(AdviceAnswer {
        choice: answer.get("choice")?.as_str()?.to_owned(),
        probabilities,
        confidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::ADVICE_ABSTAIN;

    fn questions() -> AdviceQuestionSet {
        serde_json::from_value(json!({
            "state": {"task": "Add invitations", "mode": "mvp"},
            "questions": {"size": {"instructions": "Choose the size class.",
                "criteria": {"small": "One sitting.", "large": "Several days.",
                    "ABSTAIN": "Not enough to tell."}}}
        }))
        .unwrap()
    }

    fn body(choice: &str, small: f64, large: f64, abstain: f64) -> Vec<u8> {
        json!({"model":"jev-latest","answers":{"size":{"type":"choice","choice":choice,
            "probabilities":{"small":small,"large":large,"ABSTAIN":abstain},"confidence":0.8}},
            "usage":{"input_tokens":120,"output_tokens":9}})
        .to_string()
        .into_bytes()
    }

    fn problem(raw: &[u8]) -> String {
        decode_answer(raw, &questions()).answers.unwrap_err()
    }

    #[test]
    fn request_has_the_documented_shape_and_nothing_else() {
        let request: Value =
            serde_json::from_slice(&encode_request("jev-latest", &questions())).unwrap();
        assert_eq!(
            request,
            json!({"model":"jev-latest","state":{"task":"Add invitations","mode":"mvp"},
                "questions":{"size":{"type":"choice","instructions":"Choose the size class.",
                    "criteria":{"small":"One sitting.","large":"Several days.",
                        "ABSTAIN":"Not enough to tell."}}}})
        );
    }

    #[test]
    fn valid_answer_and_abstention_are_decoded_with_usage() {
        let decoded = decode_answer(&body("small", 0.7, 0.2, 0.1), &questions());
        assert_eq!(decoded.model.as_deref(), Some("jev-latest"));
        assert_eq!(
            (decoded.input_tokens, decoded.output_tokens),
            (Some(120), Some(9))
        );
        let answers = decoded.answers.unwrap();
        assert_eq!(answers["size"].choice, "small");
        assert_eq!(answers["size"].probabilities["large"], 0.2);
        assert_eq!(answers["size"].confidence, Some(0.8));

        let abstained = decode_answer(&body(ADVICE_ABSTAIN, 0.1, 0.2, 0.7), &questions());
        assert_eq!(abstained.answers.unwrap()["size"].choice, ADVICE_ABSTAIN);
    }

    #[test]
    fn unknown_usage_stays_unknown_and_never_becomes_zero() {
        for usage in [
            json!(null),
            json!({}),
            json!({"input_tokens":-1,"output_tokens":"9"}),
        ] {
            let mut value: Value = serde_json::from_slice(&body("small", 0.7, 0.2, 0.1)).unwrap();
            value["usage"] = usage;
            let decoded = decode_answer(value.to_string().as_bytes(), &questions());
            assert_eq!((decoded.input_tokens, decoded.output_tokens), (None, None));
            assert!(decoded.answers.is_ok());
        }
        let mut value: Value = serde_json::from_slice(&body("small", 0.7, 0.2, 0.1)).unwrap();
        value.as_object_mut().unwrap().remove("usage");
        value.as_object_mut().unwrap().remove("model");
        value["answers"]["size"]
            .as_object_mut()
            .unwrap()
            .remove("confidence");
        let decoded = decode_answer(value.to_string().as_bytes(), &questions());
        assert_eq!(
            (decoded.model, decoded.input_tokens, decoded.output_tokens),
            (None, None, None)
        );
        assert_eq!(decoded.answers.unwrap()["size"].confidence, None);
    }

    #[test]
    fn duplicate_keys_and_non_objects_are_rejected() {
        let duplicate = br#"{"model":"jev-latest","answers":{"size":{"type":"choice","choice":"small","choice":"large","probabilities":{"small":0.7,"large":0.2,"ABSTAIN":0.1}}}}"#;
        assert!(problem(duplicate).contains("unique keys"));
        assert!(problem(b"malformed { bytes").contains("unique keys"));
        assert!(problem(b"[]").contains("not a JSON object"));
        assert!(problem(br#"{"model":"jev-latest"}"#).contains("no `answers` object"));
    }

    #[test]
    fn answers_breaking_an_acceptance_rule_are_rejected() {
        assert!(problem(&body("small", 0.7, 0.2, 0.2)).contains("do not sum to 1"));
        assert!(problem(&body("large", 0.7, 0.2, 0.1)).contains("not the most probable"));
        assert!(problem(&body("huge", 0.7, 0.2, 0.1)).contains("was not asked"));
        let unknown = br#"{"answers":{"size":{"type":"choice","choice":"small","probabilities":{"small":0.6,"large":0.2,"ABSTAIN":0.1,"huge":0.1}}}}"#;
        assert!(problem(unknown).contains("exactly the criteria asked"));
        let missing = br#"{"answers":{"other":{"type":"choice","choice":"small","probabilities":{"small":1.0}}}}"#;
        assert!(problem(missing).contains("has no answer"));
        for malformed in [
            br#"{"answers":{"size":{"type":"text","choice":"small","probabilities":{"small":0.7,"large":0.2,"ABSTAIN":0.1}}}}"#.as_slice(),
            br#"{"answers":{"size":{"type":"choice","choice":7,"probabilities":{"small":0.7,"large":0.2,"ABSTAIN":0.1}}}}"#,
            br#"{"answers":{"size":{"type":"choice","choice":"small","probabilities":{"small":"0.7","large":0.2,"ABSTAIN":0.1}}}}"#,
            br#"{"answers":{"size":{"type":"choice","choice":"small","probabilities":{"small":0.7,"large":0.2,"ABSTAIN":0.1},"confidence":"high"}}}"#,
            br#"{"answers":{"size":"small"}}"#,
        ] {
            assert!(problem(malformed).contains("is malformed"));
        }
    }
}
