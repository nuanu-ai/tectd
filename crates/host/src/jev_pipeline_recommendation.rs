//! Pure native TypeSafe wire for a saved, digest-bound pipeline manifest.
//! This module has no transport, phase authority, or verification authority.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Value, json};
use tect_domain::{Error, PipelineRecommendationManifest, PipelineRecommendationRanking, Result};

pub const WIRE_VERSION: &str = "tect.pipeline-typesafe-native/1";
pub const ENDPOINT_PATH: &str = "/v1/systemone";
pub const MAX_REQUEST_BYTES: usize = 512 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const CHOICE_ID: &str = "choice_v1";
const ABSTAIN: &str = "ABSTAIN";
const TOLERANCE: f64 = 1e-6;
const SCORE_LEVELS: [&str; 10] = [
    "0: incompatible with the recorded obligations",
    "1: very poor fit",
    "2: poor fit",
    "3: weak fit",
    "4: below average fit",
    "5: plausible fit with material reservations",
    "6: moderate fit",
    "7: good fit",
    "8: strong fit",
    "9: best fit with the recorded obligations",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedPipelineNativeRequest {
    pub body: Vec<u8>,
    pub model: String,
    pub manifest_digest: String,
    pub eligible_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineNativeAbstainReason {
    ProviderAbstained,
    LowChoiceProbability,
    LowChoiceConfidence,
    ChoiceScoreDisagreement,
    InsufficientScoreSeparation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedPipelineNativeResponse {
    pub ranking: PipelineRecommendationRanking,
    pub abstain_reason: Option<PipelineNativeAbstainReason>,
    pub response_model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub selected_probability: f64,
    pub choice_confidence: f64,
    /// Weighted means, in canonical manifest order.
    pub scores: Vec<(String, f64)>,
}

#[derive(Serialize)]
struct NativeRequest<'a> {
    model: &'a str,
    state: Value,
    questions: BTreeMap<String, Value>,
}

pub fn prepare_native_request(
    model: &str,
    manifest: &PipelineRecommendationManifest,
    maximum_request_bytes: usize,
) -> Result<PreparedPipelineNativeRequest> {
    manifest.validate_digest()?;
    if model.is_empty()
        || model.len() > 128
        || model.chars().any(char::is_control)
        || !(2..=8).contains(&manifest.options.len())
        || maximum_request_bytes == 0
        || maximum_request_bytes > MAX_REQUEST_BYTES
    {
        return Err(Error::InvalidArguments);
    }
    let eligible_ids = manifest
        .options
        .iter()
        .map(|option| option.id.clone())
        .collect::<Vec<_>>();
    let mut questions = BTreeMap::new();
    let mut criteria = BTreeMap::new();
    for (index, option) in manifest.options.iter().enumerate() {
        criteria.insert(option.id.clone(), json!(format!(
            "Eligible pipeline with definition version {} and digest {}. See exact obligations in state.manifest.",
            option.definition_version, option.definition_digest
        )));
        questions.insert(format!("score_v1_{index}"), json!({
            "type": "score",
            "instructions": format!("Rate only eligible pipeline ID {} against the recorded Work, selected Matrix choice, mandatory cards, and its verification obligations. Use all ten ordered levels. This is advice only; it authorizes no phase or verification claim. Wire version: {WIRE_VERSION}.", option.id),
            "criteria": SCORE_LEVELS,
        }));
    }
    criteria.insert(
        ABSTAIN.to_string(),
        json!("Evidence is insufficient or no eligible pipeline is supportable."),
    );
    questions.insert(CHOICE_ID.into(), json!({
        "type": "choice",
        "instructions": format!("Choose exactly one eligible pipeline ID or ABSTAIN. Preserve all Matrix mandatory cards and verification obligations. This is advice only; it authorizes no phase or verification claim. Wire version: {WIRE_VERSION}."),
        "criteria": criteria,
    }));
    let body = serde_json::to_vec(&NativeRequest {
        model,
        state: json!({"contract": WIRE_VERSION, "manifest_digest": manifest.digest, "manifest": manifest}),
        questions,
    }).map_err(|_| Error::InvalidArguments)?;
    if body.len() > maximum_request_bytes {
        return Err(Error::RequestTooLarge);
    }
    Ok(PreparedPipelineNativeRequest {
        body,
        model: model.into(),
        manifest_digest: manifest.digest.clone(),
        eligible_ids,
    })
}

pub fn parse_native_response(
    bytes: &[u8],
    prepared: &PreparedPipelineNativeRequest,
    maximum_response_bytes: usize,
) -> Result<ParsedPipelineNativeResponse> {
    if maximum_response_bytes == 0
        || maximum_response_bytes > MAX_RESPONSE_BYTES
        || bytes.len() > maximum_response_bytes
    {
        return Err(Error::RequestTooLarge);
    }
    if !(2..=8).contains(&prepared.eligible_ids.len())
        || prepared.eligible_ids.iter().collect::<BTreeSet<_>>().len()
            != prepared.eligible_ids.len()
    {
        return Err(Error::InvalidArguments);
    }
    let response: Value = serde_json::from_slice(bytes).map_err(|_| Error::InvalidArguments)?;
    let root = exact_object(&response, &["model", "answers", "usage"])?;
    let response_model = root
        .get("model")
        .and_then(Value::as_str)
        .ok_or(Error::InvalidArguments)?;
    if response_model.is_empty()
        || response_model.len() > 128
        || response_model.chars().any(char::is_control)
        || (prepared.model == "jev-latest" && !response_model.starts_with("jev-"))
        || (prepared.model != "jev-latest" && response_model != prepared.model)
    {
        return Err(Error::InvalidArguments);
    }
    let usage = exact_object(
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
    if answers.len() != prepared.eligible_ids.len() + 1 {
        return Err(Error::InvalidArguments);
    }
    let mut scores = Vec::with_capacity(prepared.eligible_ids.len());
    for (index, id) in prepared.eligible_ids.iter().enumerate() {
        let key = format!("score_v1_{index}");
        let answer = exact_object(
            answers.get(&key).ok_or(Error::InvalidArguments)?,
            &["type", "score", "legend", "probabilities", "confidence"],
        )?;
        if answer.get("type").and_then(Value::as_str) != Some("score") {
            return Err(Error::InvalidArguments);
        }
        let score = number(answer.get("score").ok_or(Error::InvalidArguments)?, 9.0)?;
        number(
            answer.get("confidence").ok_or(Error::InvalidArguments)?,
            1.0,
        )?;
        let legend = answer
            .get("legend")
            .and_then(Value::as_object)
            .ok_or(Error::InvalidArguments)?;
        if legend.len() != SCORE_LEVELS.len()
            || SCORE_LEVELS.iter().enumerate().any(|(level, label)| {
                legend.get(&level.to_string()).and_then(Value::as_str) != Some(*label)
            })
        {
            return Err(Error::InvalidArguments);
        }
        let support = (0..10).map(|level| level.to_string()).collect();
        let probabilities = probabilities(
            answer.get("probabilities").ok_or(Error::InvalidArguments)?,
            &support,
        )?;
        let weighted = (0..10)
            .map(|level| level as f64 * probabilities[&level.to_string()])
            .sum::<f64>();
        if (weighted - score).abs() > TOLERANCE {
            return Err(Error::InvalidArguments);
        }
        scores.push((id.clone(), weighted));
    }
    let choice = exact_object(
        answers.get(CHOICE_ID).ok_or(Error::InvalidArguments)?,
        &["type", "choice", "probabilities", "confidence"],
    )?;
    if choice.get("type").and_then(Value::as_str) != Some("choice") {
        return Err(Error::InvalidArguments);
    }
    let selected = choice
        .get("choice")
        .and_then(Value::as_str)
        .ok_or(Error::InvalidArguments)?;
    let mut support = prepared
        .eligible_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    support.insert(ABSTAIN.into());
    let probabilities = probabilities(
        choice.get("probabilities").ok_or(Error::InvalidArguments)?,
        &support,
    )?;
    let selected_probability = *probabilities.get(selected).ok_or(Error::InvalidArguments)?;
    if probabilities
        .values()
        .any(|value| *value > selected_probability)
    {
        return Err(Error::InvalidArguments);
    }
    let choice_confidence = number(
        choice.get("confidence").ok_or(Error::InvalidArguments)?,
        1.0,
    )?;
    let mut ranked = scores.clone();
    ranked.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    let reason = if selected == ABSTAIN {
        Some(PipelineNativeAbstainReason::ProviderAbstained)
    } else if selected_probability < 0.70 {
        Some(PipelineNativeAbstainReason::LowChoiceProbability)
    } else if choice_confidence < 0.70 {
        Some(PipelineNativeAbstainReason::LowChoiceConfidence)
    } else if selected != ranked[0].0 {
        Some(PipelineNativeAbstainReason::ChoiceScoreDisagreement)
    } else if ranked.windows(2).any(|pair| pair[0].1 - pair[1].1 <= 0.10) {
        Some(PipelineNativeAbstainReason::InsufficientScoreSeparation)
    } else {
        None
    };
    let ranking = if reason.is_some() {
        PipelineRecommendationRanking::Abstained
    } else {
        PipelineRecommendationRanking::Ranked {
            ranked_ids: ranked.into_iter().map(|(id, _)| id).collect(),
        }
    };
    Ok(ParsedPipelineNativeResponse {
        ranking,
        abstain_reason: reason,
        response_model: response_model.into(),
        input_tokens,
        output_tokens,
        selected_probability,
        choice_confidence,
        scores,
    })
}

fn exact_object<'a>(value: &'a Value, keys: &[&str]) -> Result<&'a serde_json::Map<String, Value>> {
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
    if (result.values().sum::<f64>() - 1.0).abs() > TOLERANCE {
        return Err(Error::InvalidArguments);
    }
    Ok(result)
}

#[cfg(test)]
#[path = "jev_pipeline_recommendation_tests.rs"]
mod tests;
