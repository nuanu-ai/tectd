//! Pure TypeSafe Choice/Score Matrix wire. No transport or ranking policy.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Value, json};
use tect_application::{MAX_PREPARED_MATRIX_BODY_BYTES, MatrixProviderRequest};
use tect_domain::{
    Error, MatrixAdviceEligibility, NativeMatrixCandidateScore, NativeMatrixChoice,
    NativeMatrixRankingSignals, Result,
};

use super::wire::{self, MatrixRankingBinding};

pub const NATIVE_MATRIX_WIRE_VERSION: &str = "tect.matrix-typesafe-native/1";
pub const NATIVE_MATRIX_ENDPOINT_PATH: &str = "/v1/systemone";
const CHOICE_QUESTION_ID: &str = "choice_v1";
const ABSTAIN_TOKEN: &str = "ABSTAIN";
const DISTRIBUTION_TOLERANCE: f64 = 1e-6;
const WEIGHTED_SCORE_TOLERANCE: f64 = 1e-6;
const SCORE_LEVELS: [&str; 10] = [
    "0: incompatible with the recorded facts",
    "1: very poor fit",
    "2: poor fit",
    "3: weak fit",
    "4: below average fit",
    "5: plausible fit with material reservations",
    "6: moderate fit",
    "7: good fit",
    "8: strong fit",
    "9: best fit with the recorded facts",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedNativeMatrixRequest {
    pub body: Vec<u8>,
    pub model: String,
    pub binding: MatrixRankingBinding,
    pub eligibility: MatrixAdviceEligibility,
    /// Stable token order follows canonical candidate ID order.
    pub token_to_candidate_id: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedNativeMatrixResponse {
    pub signals: NativeMatrixRankingSignals,
    pub response_model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Serialize)]
struct NativeRequest<'a> {
    model: &'a str,
    state: Value,
    questions: BTreeMap<String, Value>,
}

/// Prepare one native request from the application's verified, bound material.
/// The legacy wire is used only to recheck the accepted binding and digests.
pub fn prepare_native_request(
    model: &str,
    request: &MatrixProviderRequest,
    maximum_request_bytes: usize,
) -> Result<PreparedNativeMatrixRequest> {
    if model != request.model_configuration().model
        || maximum_request_bytes == 0
        || maximum_request_bytes > MAX_PREPARED_MATRIX_BODY_BYTES
    {
        return Err(Error::InvalidArguments);
    }
    let checked = wire::prepare_verified_request(model, request, MAX_PREPARED_MATRIX_BODY_BYTES)?;
    let MatrixAdviceEligibility::EligibleForAdvice { candidate_ids } = &checked.eligibility else {
        return Err(Error::InvalidArguments);
    };
    if !(2..=5).contains(&candidate_ids.len()) {
        return Err(Error::InvalidArguments);
    }
    let choice_set = request
        .revision()
        .choice_set
        .as_ref()
        .ok_or(Error::InvalidArguments)?;
    let canonical_choice_set = canonical_choice_set(choice_set);
    let mut questions = BTreeMap::new();
    let mut token_to_candidate_id = BTreeMap::new();
    let mut candidates = BTreeMap::new();
    let mut criteria = BTreeMap::new();
    for (index, candidate) in canonical_choice_set.candidates.iter().enumerate() {
        let token = format!("C{index}");
        token_to_candidate_id.insert(token.clone(), candidate.candidate_id.clone());
        candidates.insert(token.clone(), candidate);
        criteria.insert(
            token.clone(),
            json!({
                "candidate_title": candidate.title,
                "candidate_approach": candidate.approach,
            }),
        );
        questions.insert(
            format!("score_v1_{token}"),
            json!({
                "type": "score",
                "instructions": format!("Rate only candidate token {token} for suitability against the recorded Matrix facts and decision question. Use all ten ordered levels. Higher means more suitable. Do not infer missing evidence or treat mandatory cards as optional. This is advice only and authorizes no action. Wire version: {NATIVE_MATRIX_WIRE_VERSION}."),
                "criteria": SCORE_LEVELS,
            }),
        );
    }
    criteria.insert(ABSTAIN_TOKEN.into(), json!(
        "Evidence is insufficient, candidates cannot be distinguished, or no candidate is supportable."
    ));
    questions.insert(CHOICE_QUESTION_ID.into(), json!({
        "type": "choice",
        "instructions": format!("Choose exactly one candidate token as the best fit for the recorded facts, or ABSTAIN when evidence is insufficient, candidates cannot be distinguished, or no candidate is supportable. Use only the supplied facts. Mandatory cards remain mandatory. This is advice only and authorizes no action. Wire version: {NATIVE_MATRIX_WIRE_VERSION}."),
        "criteria": criteria,
    }));
    let state = json!({
        "contract": NATIVE_MATRIX_WIRE_VERSION,
        "binding": checked.binding,
        "input": request.revision().input,
        "composition": request.composition(),
        "choice_set": canonical_choice_set,
        "candidate_tokens": candidates,
    });
    let body = serde_json::to_vec(&NativeRequest {
        model,
        state,
        questions,
    })
    .map_err(|_| Error::InvalidArguments)?;
    if body.len() > maximum_request_bytes {
        return Err(Error::RequestTooLarge);
    }
    Ok(PreparedNativeMatrixRequest {
        body,
        model: model.into(),
        binding: checked.binding,
        eligibility: checked.eligibility,
        token_to_candidate_id,
    })
}

/// Parse exactly one native TypeSafe response. Malformed provider JSON is an
/// error; valid low-confidence signals are passed to the domain policy.
pub fn parse_native_response(
    bytes: &[u8],
    prepared: &PreparedNativeMatrixRequest,
    maximum_response_bytes: usize,
) -> Result<ParsedNativeMatrixResponse> {
    if maximum_response_bytes == 0
        || maximum_response_bytes > super::MAX_MATRIX_RESPONSE_BYTES
        || bytes.len() > maximum_response_bytes
    {
        return Err(Error::RequestTooLarge);
    }
    let response =
        crate::jev_json::decode_unique_json(bytes).map_err(|_| Error::InvalidArguments)?;
    let root = object_with_keys(&response, &["model", "answers", "usage"])?;
    let response_model = root
        .get("model")
        .and_then(Value::as_str)
        .ok_or(Error::InvalidArguments)?;
    // An alias request may resolve to a concrete model version in the response.
    if response_model.is_empty()
        || response_model.len() > 128
        || response_model.chars().any(char::is_control)
        || (prepared.model == "jev-latest" && !response_model.starts_with("jev-"))
        || (prepared.model != "jev-latest" && response_model != prepared.model)
    {
        return Err(Error::InvalidArguments);
    }
    let usage = object_with_keys(
        root.get("usage").ok_or(Error::InvalidArguments)?,
        &["input_tokens", "output_tokens"],
    )?;
    let input_tokens = usage
        .get("input_tokens")
        .and_then(Value::as_u64)
        .ok_or(Error::InvalidArguments)?;
    let output_tokens = usage
        .get("output_tokens")
        .and_then(Value::as_u64)
        .ok_or(Error::InvalidArguments)?;
    let answers = root
        .get("answers")
        .and_then(Value::as_object)
        .ok_or(Error::InvalidArguments)?;
    if answers.len() != prepared.token_to_candidate_id.len() + 1 {
        return Err(Error::InvalidArguments);
    }
    let mut candidate_scores = Vec::with_capacity(prepared.token_to_candidate_id.len());
    for (token, candidate_id) in &prepared.token_to_candidate_id {
        let key = format!("score_v1_{token}");
        let answer = object_with_keys(
            answers.get(&key).ok_or(Error::InvalidArguments)?,
            &["type", "score", "legend", "probabilities", "confidence"],
        )?;
        if answer.get("type").and_then(Value::as_str) != Some("score") {
            return Err(Error::InvalidArguments);
        }
        let score = number(answer.get("score").ok_or(Error::InvalidArguments)?, 9.0)?;
        let confidence = number(
            answer.get("confidence").ok_or(Error::InvalidArguments)?,
            1.0,
        )?;
        let legend = answer
            .get("legend")
            .and_then(Value::as_object)
            .ok_or(Error::InvalidArguments)?;
        if legend.len() != SCORE_LEVELS.len() {
            return Err(Error::InvalidArguments);
        }
        for (level, description) in SCORE_LEVELS.iter().enumerate() {
            if legend.get(&level.to_string()).and_then(Value::as_str) != Some(*description) {
                return Err(Error::InvalidArguments);
            }
        }
        let support = (0..10).map(|n| n.to_string()).collect::<BTreeSet<_>>();
        let probabilities = probabilities(
            answer.get("probabilities").ok_or(Error::InvalidArguments)?,
            &support,
        )?;
        let weighted = (0..10)
            .map(|level| level as f64 * probabilities[&level.to_string()])
            .sum::<f64>();
        if (weighted - score).abs() > WEIGHTED_SCORE_TOLERANCE {
            return Err(Error::InvalidArguments);
        }
        candidate_scores.push(NativeMatrixCandidateScore {
            candidate_id: candidate_id.clone(),
            score: weighted,
            answer_confidence: confidence,
        });
    }
    let answer = object_with_keys(
        answers
            .get(CHOICE_QUESTION_ID)
            .ok_or(Error::InvalidArguments)?,
        &["type", "choice", "probabilities", "confidence"],
    )?;
    if answer.get("type").and_then(Value::as_str) != Some("choice") {
        return Err(Error::InvalidArguments);
    }
    let selected = answer
        .get("choice")
        .and_then(Value::as_str)
        .ok_or(Error::InvalidArguments)?;
    let mut support = prepared
        .token_to_candidate_id
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    support.insert(ABSTAIN_TOKEN.into());
    let probabilities = probabilities(
        answer.get("probabilities").ok_or(Error::InvalidArguments)?,
        &support,
    )?;
    let selected_probability = *probabilities.get(selected).ok_or(Error::InvalidArguments)?;
    if probabilities
        .values()
        .any(|probability| *probability > selected_probability)
    {
        return Err(Error::InvalidArguments);
    }
    let choice = if selected == ABSTAIN_TOKEN {
        NativeMatrixChoice::Abstain
    } else {
        NativeMatrixChoice::Candidate(
            prepared
                .token_to_candidate_id
                .get(selected)
                .ok_or(Error::InvalidArguments)?
                .clone(),
        )
    };
    let confidence = number(
        answer.get("confidence").ok_or(Error::InvalidArguments)?,
        1.0,
    )?;
    Ok(ParsedNativeMatrixResponse {
        signals: NativeMatrixRankingSignals {
            candidate_scores,
            choice,
            choice_confidence: confidence,
            choice_selected_answer_probability: selected_probability,
        },
        response_model: response_model.into(),
        input_tokens,
        output_tokens,
    })
}

fn object_with_keys<'a>(
    value: &'a Value,
    keys: &[&str],
) -> Result<&'a serde_json::Map<String, Value>> {
    let object = value.as_object().ok_or(Error::InvalidArguments)?;
    if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
        return Err(Error::InvalidArguments);
    }
    Ok(object)
}

fn number(value: &Value, maximum: f64) -> Result<f64> {
    let value = value.as_f64().ok_or(Error::InvalidArguments)?;
    if !value.is_finite() || !(0.0..=maximum).contains(&value) {
        return Err(Error::InvalidArguments);
    }
    Ok(value)
}

fn probabilities(value: &Value, support: &BTreeSet<String>) -> Result<BTreeMap<String, f64>> {
    let object = value.as_object().ok_or(Error::InvalidArguments)?;
    if object.len() != support.len() || object.keys().any(|key| !support.contains(key)) {
        return Err(Error::InvalidArguments);
    }
    let mut result = BTreeMap::new();
    for key in support {
        result.insert(
            key.clone(),
            number(object.get(key).ok_or(Error::InvalidArguments)?, 1.0)?,
        );
    }
    if (result.values().sum::<f64>() - 1.0).abs() > DISTRIBUTION_TOLERANCE {
        return Err(Error::InvalidArguments);
    }
    Ok(result)
}

fn canonical_choice_set(
    choice_set: &tect_domain::EngineeringChoiceSet,
) -> tect_domain::EngineeringChoiceSet {
    let mut canonical = choice_set.clone();
    canonical
        .candidates
        .sort_by(|a, b| a.candidate_id.cmp(&b.candidate_id));
    for candidate in &mut canonical.candidates {
        candidate.assumption_fact_ids.sort();
    }
    canonical
}

#[cfg(test)]
#[path = "native_wire_tests.rs"]
mod tests;
