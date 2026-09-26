use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use tect_domain::{
    ConfidenceBasisPoints, NormalizedScopeAdviceAnswer, NormalizedScopeAdviceAnswers,
    ScopeAdviceChoice, ScopeAdviceRequest, ScopeAdviceScoreBand, ScopeDecompositionAlternative,
};

const SCORE_LEGEND: [&str; 4] = ["conflict", "weak_fit", "fit", "strong_fit"];

#[derive(Serialize)]
struct RequestBody<'a> {
    model: &'a str,
    state: ProviderState<'a>,
    questions: BTreeMap<String, Question>,
}

#[derive(Serialize)]
struct ProviderState<'a> {
    request: &'a ScopeAdviceRequest,
    emitted: &'a [ScopeDecompositionAlternative],
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Question {
    Choice {
        instructions: &'static str,
        criteria: BTreeMap<&'static str, &'static str>,
    },
    Score {
        instructions: &'static str,
        criteria: [&'static str; 4],
    },
}

pub(super) fn serialize_request(
    model: &str,
    request: &ScopeAdviceRequest,
    emitted: &[ScopeDecompositionAlternative],
) -> std::result::Result<Vec<u8>, ()> {
    if !emitted.is_empty()
        && (request.alternatives.len() != emitted.len()
            || request
                .alternatives
                .iter()
                .zip(emitted)
                .any(|(bound, material)| {
                    bound.id != material.id
                        || bound.kind != material.kind
                        || bound.material_digest != material.material_digest
                }))
    {
        return Err(());
    }
    let expected = request
        .alternatives
        .iter()
        .map(|value| &value.id)
        .collect::<BTreeSet<_>>();
    let actual = request
        .questions
        .iter()
        .map(|value| &value.alternative_id)
        .collect::<BTreeSet<_>>();
    if request.questions.len() != expected.len()
        || actual != expected
        || request
            .questions
            .iter()
            .any(|value| !value.require_choice || !value.require_score)
    {
        return Err(());
    }
    let mut questions = BTreeMap::new();
    for alternative in &request.alternatives {
        questions.insert(
            format!("choice_{}", alternative.id.0),
            Question::Choice {
                instructions: "Choose whether this eligible alternative is preferred using its matching ID and emitted material in state.",
                criteria: BTreeMap::from([
                    ("PREFERRED", "preferred for the supplied scope"),
                    ("NON_PREFERRED", "not preferred for the supplied scope"),
                ]),
            },
        );
        questions.insert(
            format!("score_{}", alternative.id.0),
            Question::Score {
                instructions: "Score this eligible alternative using its matching ID and emitted material in state.",
                criteria: SCORE_LEGEND,
            },
        );
    }
    serde_json::to_vec(&RequestBody {
        model,
        state: ProviderState { request, emitted },
        questions,
    })
    .map_err(|_| ())
}

pub(super) struct ParsedResponse {
    pub answers: NormalizedScopeAdviceAnswers,
}

pub(super) fn parse_unique_json(bytes: &[u8]) -> std::result::Result<Value, ()> {
    crate::jev_json::decode_unique_json(bytes).map_err(|_| ())
}

pub(super) fn parse_response(
    bytes: &[u8],
    model: &str,
    request: &ScopeAdviceRequest,
) -> std::result::Result<ParsedResponse, ()> {
    let value = parse_unique_json(bytes)?;
    let root = object(value)?;
    exact_keys(&root, &["answers", "model", "usage"])?;
    if string(root.get("model"))? != model {
        return Err(());
    }
    let answers = object(root.get("answers").cloned().ok_or(())?)?;
    let expected_keys = request
        .alternatives
        .iter()
        .flat_map(|value| {
            [
                format!("choice_{}", value.id.0),
                format!("score_{}", value.id.0),
            ]
        })
        .collect::<BTreeSet<_>>();
    if answers.keys().cloned().collect::<BTreeSet<_>>() != expected_keys {
        return Err(());
    }
    let mut normalized = Vec::with_capacity(request.alternatives.len());
    for alternative in &request.alternatives {
        let choice = object(
            answers
                .get(&format!("choice_{}", alternative.id.0))
                .cloned()
                .ok_or(())?,
        )?;
        exact_keys(&choice, &["choice", "confidence", "probabilities", "type"])?;
        if string(choice.get("type"))? != "choice" {
            return Err(());
        }
        let choice_value = match string(choice.get("choice"))? {
            "PREFERRED" => ScopeAdviceChoice::Preferred,
            "NON_PREFERRED" => ScopeAdviceChoice::NonPreferred,
            _ => return Err(()),
        };
        let choice_confidence = confidence(choice.get("confidence"))?;
        probabilities(choice.get("probabilities"), &["NON_PREFERRED", "PREFERRED"])?;

        let score = object(
            answers
                .get(&format!("score_{}", alternative.id.0))
                .cloned()
                .ok_or(())?,
        )?;
        exact_keys(
            &score,
            &["confidence", "legend", "probabilities", "score", "type"],
        )?;
        if string(score.get("type"))? != "score" {
            return Err(());
        }
        let raw_score = finite_unit(score.get("score"), 3.0)?;
        let score_value = match raw_score.round() as u8 {
            0 => ScopeAdviceScoreBand::Conflict,
            1 => ScopeAdviceScoreBand::WeakFit,
            2 => ScopeAdviceScoreBand::Fit,
            3 => ScopeAdviceScoreBand::StrongFit,
            _ => return Err(()),
        };
        let score_confidence = confidence(score.get("confidence"))?;
        probabilities(score.get("probabilities"), &["0", "1", "2", "3"])?;
        let legend = object(score.get("legend").cloned().ok_or(())?)?;
        exact_keys(&legend, &["0", "1", "2", "3"])?;
        for (index, expected) in SCORE_LEGEND.iter().enumerate() {
            if string(legend.get(&index.to_string()))? != *expected {
                return Err(());
            }
        }
        normalized.push(NormalizedScopeAdviceAnswer {
            alternative_id: alternative.id.clone(),
            choice: choice_value,
            score: score_value,
            choice_confidence,
            score_confidence,
        });
    }
    // Usage is retained in the raw schema but decoded only by the sealed
    // accounting hook; answer interpretation supplies no counters.
    Ok(ParsedResponse {
        answers: NormalizedScopeAdviceAnswers {
            answers: normalized,
        },
    })
}

fn object(value: Value) -> std::result::Result<Map<String, Value>, ()> {
    value.as_object().cloned().ok_or(())
}

fn exact_keys(map: &Map<String, Value>, expected: &[&str]) -> std::result::Result<(), ()> {
    let actual = map.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    (actual == expected).then_some(()).ok_or(())
}

fn string(value: Option<&Value>) -> std::result::Result<&str, ()> {
    value.and_then(Value::as_str).ok_or(())
}

fn finite_unit(value: Option<&Value>, maximum: f64) -> std::result::Result<f64, ()> {
    let number = value.and_then(Value::as_f64).ok_or(())?;
    (number.is_finite() && (0.0..=maximum).contains(&number))
        .then_some(number)
        .ok_or(())
}

fn confidence(value: Option<&Value>) -> std::result::Result<ConfidenceBasisPoints, ()> {
    let value = finite_unit(value, 1.0)?;
    Ok(ConfidenceBasisPoints((value * 10_000.0).round() as u16))
}

fn probabilities(value: Option<&Value>, keys: &[&str]) -> std::result::Result<(), ()> {
    let values = object(value.cloned().ok_or(())?)?;
    exact_keys(&values, keys)?;
    let sum = values.values().try_fold(0.0, |sum, value| {
        Ok::<_, ()>(sum + finite_unit(Some(value), 1.0)?)
    })?;
    ((sum - 1.0).abs() <= 0.03).then_some(()).ok_or(())
}
