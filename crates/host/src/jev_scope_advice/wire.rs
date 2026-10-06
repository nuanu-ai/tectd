use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use tect_domain::{
    ComparativeDisposition, ConfidenceBasisPoints, NormalizedScopeAdviceAnswer,
    NormalizedScopeAdviceAnswers, ScopeAdviceChoice, ScopeAdviceRequest, ScopeAdviceScoreBand,
    ScopeDecompositionAlternative,
};

const SCORE_LEGEND: [&str; 4] = ["conflict", "weak_fit", "fit", "strong_fit"];
const CHOICE_ID: &str = "choice_v3";
const ABSTAIN: &str = "ABSTAIN";

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
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate_tokens: Option<BTreeMap<String, &'a tect_domain::ScopeAdviceAlternative>>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Question {
    Choice {
        instructions: String,
        criteria: BTreeMap<String, String>,
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
    if request.alternatives.is_empty()
        || request.alternatives.len() != expected.len()
        || request.questions.len() != expected.len()
        || actual != expected
        || request
            .questions
            .iter()
            .any(|value| !value.require_choice || !value.require_score)
    {
        return Err(());
    }
    let mut questions = BTreeMap::new();
    let mut candidates = request.alternatives.iter().collect::<Vec<_>>();
    candidates.sort_by(|a, b| a.id.0.cmp(&b.id.0));
    let mut candidate_tokens = BTreeMap::new();
    let mut criteria = BTreeMap::new();
    for (index, alternative) in candidates.into_iter().enumerate() {
        let token = format!("C{index}");
        candidate_tokens.insert(token.clone(), alternative);
        criteria.insert(token, format!("eligible alternative {}", alternative.id.0));
        questions.insert(
            format!("score_{}", alternative.id.0),
            Question::Score {
                instructions: "Score this eligible alternative using its matching ID and emitted material in state.",
                criteria: SCORE_LEGEND,
            },
        );
    }
    criteria.insert(
        ABSTAIN.into(),
        "No alternative is supportable as preferred.".into(),
    );
    questions.insert(CHOICE_ID.into(), Question::Choice {
        instructions: "Choose exactly one eligible candidate token as preferred, or ABSTAIN when none is supportable. Use the token binding in state and supply probabilities for every token. This advice authorizes no disposition or execution.".into(),
        criteria,
    });
    serde_json::to_vec(&RequestBody {
        model,
        state: ProviderState {
            request,
            emitted,
            candidate_tokens: Some(candidate_tokens),
        },
        questions,
    })
    .map_err(|_| ())
}

pub(super) fn serialize_request_v2(
    model: &str,
    request: &ScopeAdviceRequest,
    emitted: &[ScopeDecompositionAlternative],
) -> std::result::Result<Vec<u8>, ()> {
    // Retain the original byte-for-byte v2 request shape for sealed receipts.
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
        questions.insert(format!("choice_{}", alternative.id.0), Question::Choice {
            instructions: "Choose whether this eligible alternative is preferred using its matching ID and emitted material in state.".into(),
            criteria: BTreeMap::from([("PREFERRED".into(), "preferred for the supplied scope".into()), ("NON_PREFERRED".into(), "not preferred for the supplied scope".into())]),
        });
        questions.insert(format!("score_{}", alternative.id.0), Question::Score {
            instructions: "Score this eligible alternative using its matching ID and emitted material in state.", criteria: SCORE_LEGEND,
        });
    }
    serde_json::to_vec(&RequestBody {
        model,
        state: ProviderState {
            request,
            emitted,
            candidate_tokens: None,
        },
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

pub(super) fn parse_response_v2(
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
            comparative_disposition: None,
        },
    })
}

pub(super) fn parse_response(
    bytes: &[u8],
    model: &str,
    request: &ScopeAdviceRequest,
) -> std::result::Result<ParsedResponse, ()> {
    let root = object(parse_unique_json(bytes)?)?;
    exact_keys(&root, &["answers", "model", "usage"])?;
    if string(root.get("model"))? != model {
        return Err(());
    }
    let answers = object(root.get("answers").cloned().ok_or(())?)?;
    let mut expected = request
        .alternatives
        .iter()
        .map(|a| format!("score_{}", a.id.0))
        .collect::<BTreeSet<_>>();
    expected.insert(CHOICE_ID.into());
    if answers.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(());
    }
    let mut ordered = request.alternatives.iter().collect::<Vec<_>>();
    ordered.sort_by(|a, b| a.id.0.cmp(&b.id.0));
    let tokens = ordered
        .iter()
        .enumerate()
        .map(|(i, a)| (format!("C{i}"), &a.id))
        .collect::<BTreeMap<_, _>>();
    let choice = object(answers.get(CHOICE_ID).cloned().ok_or(())?)?;
    exact_keys(&choice, &["choice", "confidence", "probabilities", "type"])?;
    if string(choice.get("type"))? != "choice" {
        return Err(());
    }
    let selected = string(choice.get("choice"))?;
    let choice_confidence = confidence(choice.get("confidence"))?;
    let mut support = tokens.keys().map(String::as_str).collect::<Vec<_>>();
    support.push(ABSTAIN);
    let choice_probabilities = probability_values(choice.get("probabilities"), &support, 1e-6)?;
    let selected_probability = *choice_probabilities.get(selected).ok_or(())?;
    if choice_probabilities
        .values()
        .any(|p| *p > selected_probability)
    {
        return Err(());
    }
    let selected_id = if selected == ABSTAIN {
        None
    } else {
        Some(*tokens.get(selected).ok_or(())?)
    };
    let mut normalized = Vec::with_capacity(request.alternatives.len());
    let mut score_bands = BTreeMap::new();
    for alternative in &request.alternatives {
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
        probability_values(score.get("probabilities"), &["0", "1", "2", "3"], 1e-6)?;
        let legend = object(score.get("legend").cloned().ok_or(())?)?;
        exact_keys(&legend, &["0", "1", "2", "3"])?;
        for (index, expected) in SCORE_LEGEND.iter().enumerate() {
            if string(legend.get(&index.to_string()))? != *expected {
                return Err(());
            }
        }
        score_bands.insert(&alternative.id, score_value);
        normalized.push(NormalizedScopeAdviceAnswer {
            alternative_id: alternative.id.clone(),
            choice: if selected_id == Some(&alternative.id) {
                ScopeAdviceChoice::Preferred
            } else {
                ScopeAdviceChoice::NonPreferred
            },
            score: score_value,
            choice_confidence,
            score_confidence,
        });
    }
    if let Some(selected_id) = selected_id {
        let selected_band = score_bands.get(selected_id).ok_or(())?.ordinal();
        if score_bands
            .values()
            .any(|band| band.ordinal() > selected_band)
        {
            return Err(());
        }
    }
    Ok(ParsedResponse {
        answers: NormalizedScopeAdviceAnswers {
            answers: normalized,
            comparative_disposition: Some(match selected_id {
                Some(id) => ComparativeDisposition::Selected(id.clone()),
                None => ComparativeDisposition::Abstain,
            }),
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
    probability_values(value, keys, 0.03).map(|_| ())
}

fn probability_values(
    value: Option<&Value>,
    keys: &[&str],
    tolerance: f64,
) -> std::result::Result<BTreeMap<String, f64>, ()> {
    let values = object(value.cloned().ok_or(())?)?;
    exact_keys(&values, keys)?;
    let parsed = values
        .iter()
        .map(|(k, v)| Ok((k.clone(), finite_unit(Some(v), 1.0)?)))
        .collect::<std::result::Result<BTreeMap<_, _>, ()>>()?;
    let sum = parsed.values().sum::<f64>();
    ((sum - 1.0).abs() <= tolerance).then_some(parsed).ok_or(())
}

#[cfg(test)]
mod tests;
