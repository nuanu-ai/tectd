//! Pure System One Choice codec. Call response decoders only after usage sealing.
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use tect_application::{
    AntiBloatRankingMaterial, AntiBloatRankingOutcome, AntiBloatUsage, anti_bloat_material_sha256,
};
use tect_domain::{Error, Result};

pub const CHOICE_WIRE_VERSION: &str = "tect.anti-bloat-typesafe-choice/1";
const QUESTION: &str = "anti_bloat_order_v1";
const ABSTAIN: &str = "ABSTAIN";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedChoiceRequest {
    body: Vec<u8>,
    model: String,
    material_sha256: String,
    token_to_finding_id: BTreeMap<String, String>,
}

impl PreparedChoiceRequest {
    pub fn body(&self) -> &[u8] {
        &self.body
    }
    pub fn model(&self) -> &str {
        &self.model
    }
    pub fn material_sha256(&self) -> &str {
        &self.material_sha256
    }
}

/// Rebuild decoder authority from the exact persisted request, never adapter memory.
pub fn restore_choice_request(
    permit: &tect_application::AntiBloatSendPermit,
    expected_model: &str,
    expected_adapter_identity: &str,
    provider_binding_digest: &str,
    maximum_bytes: usize,
) -> Result<PreparedChoiceRequest> {
    use sha2::{Digest, Sha256};
    if permit.request.adapter_identity != expected_adapter_identity
        || expected_adapter_identity.is_empty()
        || permit.request.sha256 != format!("{:x}", Sha256::digest(&permit.request.bytes))
    {
        return Err(Error::InputConflict);
    }
    let value = raw_value(&permit.request.bytes, maximum_bytes)?;
    let state = &value["state"];
    let binding = &state["binding"];
    let saved = tect_application::StoredAntiBloatReview {
        review_id: serde_json::from_value(binding["review_id"].clone())
            .map_err(|_| Error::InvalidArguments)?,
        workspace_id: serde_json::from_value(binding["workspace_id"].clone())
            .map_err(|_| Error::InvalidArguments)?,
        actor_id: serde_json::from_value(binding["actor_id"].clone())
            .map_err(|_| Error::InvalidArguments)?,
        input: serde_json::from_value(state["input"].clone())
            .map_err(|_| Error::InvalidArguments)?,
        review: serde_json::from_value(state["review"].clone())
            .map_err(|_| Error::InvalidArguments)?,
        state: tect_application::AntiBloatAttemptState::Prepared,
    };
    let eligible_ids: Vec<String> = serde_json::from_value(state["eligible_ids"].clone())
        .map_err(|_| Error::InvalidArguments)?;
    let prepared = prepare_choice_request(
        expected_model,
        provider_binding_digest,
        &AntiBloatRankingMaterial {
            saved: &saved,
            eligible_ids: &eligible_ids,
        },
        maximum_bytes,
    )?;
    if saved.review_id != permit.review_id
        || saved.review_id.is_nil()
        || prepared.body != permit.request.bytes
        || prepared.material_sha256 != permit.request.material_sha256
    {
        return Err(Error::InputConflict);
    }
    Ok(prepared)
}

pub fn prepare_choice_request(
    model: &str,
    provider_binding_digest: &str,
    material: &AntiBloatRankingMaterial<'_>,
    maximum_bytes: usize,
) -> Result<PreparedChoiceRequest> {
    if model.is_empty()
        || model.len() > 128
        || model.chars().any(char::is_control)
        || maximum_bytes == 0
        || provider_binding_digest.len() != 64
        || !provider_binding_digest
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error::InvalidArguments);
    }
    let findings = material
        .saved
        .review
        .findings
        .iter()
        .filter(|f| f.rankable)
        .map(|f| (f.id.clone(), f))
        .collect::<BTreeMap<_, _>>();
    let eligible = material
        .eligible_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if findings.is_empty()
        || eligible.len() != material.eligible_ids.len()
        || findings.len()
            != material
                .saved
                .review
                .findings
                .iter()
                .filter(|f| f.rankable)
                .count()
        || eligible != findings.keys().cloned().collect()
    {
        return Err(Error::InvalidArguments);
    }
    let token_to_finding_id = findings
        .keys()
        .enumerate()
        .map(|(i, id)| (format!("R{i}"), id.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut criteria = token_to_finding_id
        .iter()
        .map(|(token, id)| (token.clone(), json!(findings[id])))
        .collect::<BTreeMap<_, _>>();
    criteria.insert(
        ABSTAIN.into(),
        json!("Insufficient evidence or findings cannot be distinctly prioritised."),
    );
    let material_sha256 = anti_bloat_material_sha256(material.saved)?;
    let body = serde_json::to_vec(&json!({
        "model": model,
        "state": {
            "contract": CHOICE_WIRE_VERSION,
            "provider_binding_digest": provider_binding_digest,
            "binding": {"review_id": material.saved.review_id, "workspace_id": material.saved.workspace_id, "actor_id": material.saved.actor_id, "material_sha256": material_sha256},
            "input": material.saved.input, "review": material.saved.review,
            "eligible_ids": eligible, "finding_tokens": token_to_finding_id,
        },
        "questions": { QUESTION: {"type": "choice", "instructions": format!("Prioritise actionable source-relative anti-bloat findings for caller review. Choose the highest priority supplied finding token, or ABSTAIN when evidence is insufficient or findings cannot be distinguished. Supply probabilities for every token. Preserve every source requirement and mandatory policy obligation. This is advisory prioritisation only: it never authorizes deleting source requirements, applying changes, or claiming proof or implementation. Use only the immutable supplied context. Wire version: {CHOICE_WIRE_VERSION}."), "criteria": criteria}}
    })).map_err(|_| Error::InvalidArguments)?;
    if body.len() > maximum_bytes {
        return Err(Error::RequestTooLarge);
    }
    Ok(PreparedChoiceRequest {
        body,
        model: model.into(),
        material_sha256,
        token_to_finding_id,
    })
}

fn raw_value(raw: &[u8], maximum_bytes: usize) -> Result<Value> {
    if maximum_bytes == 0 || raw.len() > maximum_bytes {
        return Err(Error::RequestTooLarge);
    }
    crate::jev_json::decode_unique_json(raw).map_err(|_| Error::InvalidArguments)
}

/// Invalid provider content is a typed terminal outcome, never a partial ranking.
pub fn parse_choice_response(
    raw: &[u8],
    prepared: &PreparedChoiceRequest,
    maximum_bytes: usize,
) -> AntiBloatRankingOutcome {
    parse(raw, prepared, maximum_bytes).unwrap_or(AntiBloatRankingOutcome::InvalidResponse)
}

fn parse(
    raw: &[u8],
    prepared: &PreparedChoiceRequest,
    maximum_bytes: usize,
) -> Result<AntiBloatRankingOutcome> {
    let value = raw_value(raw, maximum_bytes)?;
    let root = value.as_object().ok_or(Error::InvalidArguments)?;
    if root
        .keys()
        .any(|k| !["model", "answers", "usage"].contains(&k.as_str()))
        || root.get("model").and_then(Value::as_str) != Some(prepared.model.as_str())
        || prepared.token_to_finding_id.is_empty()
    {
        return Err(Error::InvalidArguments);
    }
    let answers = root
        .get("answers")
        .and_then(Value::as_object)
        .ok_or(Error::InvalidArguments)?;
    if answers.len() != 1 {
        return Err(Error::InvalidArguments);
    }
    let answer = answers
        .get(QUESTION)
        .and_then(Value::as_object)
        .ok_or(Error::InvalidArguments)?;
    if answer
        .keys()
        .any(|k| !["type", "choice", "probabilities", "confidence"].contains(&k.as_str()))
        || answer.get("type").and_then(Value::as_str) != Some("choice")
    {
        return Err(Error::InvalidArguments);
    }
    if let Some(confidence) = answer.get("confidence") {
        probability(confidence)?;
    }
    let selected = answer
        .get("choice")
        .and_then(Value::as_str)
        .ok_or(Error::InvalidArguments)?;
    let probabilities = answer
        .get("probabilities")
        .and_then(Value::as_object)
        .ok_or(Error::InvalidArguments)?;
    let mut expected = prepared
        .token_to_finding_id
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    expected.insert(ABSTAIN.into());
    if expected != probabilities.keys().cloned().collect() {
        return Err(Error::InvalidArguments);
    }
    let probabilities = probabilities
        .iter()
        .map(|(k, v)| Ok((k.clone(), probability(v)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    if (probabilities.values().sum::<f64>() - 1.0).abs() > 1e-6 {
        return Err(Error::InvalidArguments);
    }
    let selected_probability = probabilities.get(selected).ok_or(Error::InvalidArguments)?;
    if probabilities.values().any(|p| p > selected_probability) {
        return Err(Error::InvalidArguments);
    }
    let mut ranked = prepared
        .token_to_finding_id
        .iter()
        .map(|(token, id)| (id.clone(), probabilities[token]))
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    if selected == ABSTAIN
        || probabilities
            .values()
            .filter(|p| *p == selected_probability)
            .count()
            > 1
        || ranked.windows(2).any(|w| w[0].1 == w[1].1)
    {
        return Ok(AntiBloatRankingOutcome::Abstained);
    }
    Ok(AntiBloatRankingOutcome::Ranked(
        ranked.into_iter().map(|(id, _)| id).collect(),
    ))
}

fn probability(value: &Value) -> Result<f64> {
    let number = value.as_f64().ok_or(Error::InvalidArguments)?;
    if !number.is_finite() || !(0.0..=1.0).contains(&number) {
        return Err(Error::InvalidArguments);
    }
    Ok(number)
}

/// Missing counters remain unknown. This decoder does not adjudicate ranking.
pub fn decode_choice_usage(raw: &[u8], maximum_bytes: usize) -> Result<AntiBloatUsage> {
    let value = raw_value(raw, maximum_bytes)?;
    let root = value.as_object().ok_or(Error::InvalidArguments)?;
    let Some(usage) = root.get("usage") else {
        return Ok(AntiBloatUsage {
            input_tokens: None,
            output_tokens: None,
        });
    };
    let usage = usage.as_object().ok_or(Error::InvalidArguments)?;
    let counter = |key| -> Result<Option<i64>> {
        usage
            .get(key)
            .map(|v| {
                v.as_i64()
                    .filter(|n| *n >= 0)
                    .ok_or(Error::InvalidArguments)
            })
            .transpose()
    };
    Ok(AntiBloatUsage {
        input_tokens: counter("input_tokens")?,
        output_tokens: counter("output_tokens")?,
    })
}

#[cfg(test)]
pub(crate) mod tests;
