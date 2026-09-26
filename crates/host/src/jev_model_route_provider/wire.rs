//! Pure Slice05 Choice mapping. Adviser probabilities never execute a route.
use super::MODEL_ROUTE_CHOICE_WIRE_VERSION;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use tect_application::ModelRouteUsage;
use tect_domain::{
    Error, ModelRouteRankingWireOutcome, ModelRouteRankingWireRequest, ModelRouteWireAbstainReason,
    Result, validate_model_route_ranking_outcome,
};
const QUESTION: &str = "model_route_order_v1";
const ABSTAIN: &str = "ABSTAIN";

pub(super) fn prepare(
    request: &ModelRouteRankingWireRequest,
    provider_binding_digest: &str,
    maximum_bytes: usize,
) -> Result<Vec<u8>> {
    request.validate()?;
    if maximum_bytes == 0
        || provider_binding_digest.len() != 64
        || !provider_binding_digest
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error::InvalidArguments);
    }
    let route_tokens = request
        .eligible_routes
        .iter()
        .enumerate()
        .map(|(i, route)| (format!("R{i}"), route.id.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut criteria = request
        .eligible_routes
        .iter()
        .enumerate()
        .map(|(i, route)| (format!("R{i}"), json!(route)))
        .collect::<BTreeMap<_, _>>();
    criteria.insert(
        ABSTAIN.into(),
        json!("No supportable distinct preference among the eligible routes."),
    );
    let bytes=serde_json::to_vec(&json!({"model":request.binding.adviser_model,"state":{"contract":MODEL_ROUTE_CHOICE_WIRE_VERSION,"provider_binding_digest":provider_binding_digest,"request":request,"route_tokens":route_tokens},"questions":{QUESTION:{"type":"choice","instructions":format!("Prioritise all supplied eligible model routes against the immutable recorded work and host evidence. Choose the most suitable route token, or ABSTAIN when no distinct preference is supportable. Supply probabilities for every token. Adviser model identity is separate from each candidate route model. This is recommendation only: do not execute a route, assume availability beyond recorded evidence, alter eligibility, or claim actual execution. Wire version: {MODEL_ROUTE_CHOICE_WIRE_VERSION}."),"criteria":criteria}}})).map_err(|_|Error::InvalidArguments)?;
    if bytes.len() > maximum_bytes {
        return Err(Error::RequestTooLarge);
    }
    Ok(bytes)
}

fn value(raw: &[u8], maximum_bytes: usize) -> Result<Value> {
    if maximum_bytes == 0 || raw.len() > maximum_bytes {
        return Err(Error::RequestTooLarge);
    }
    serde_json::from_slice(raw).map_err(|_| Error::InvalidArguments)
}
fn probability(value: &Value) -> Result<f64> {
    let number = value.as_f64().ok_or(Error::InvalidArguments)?;
    if !number.is_finite() || !(0.0..=1.0).contains(&number) {
        return Err(Error::InvalidArguments);
    }
    Ok(number)
}

pub(super) fn parse(
    request: &ModelRouteRankingWireRequest,
    raw: &[u8],
    maximum_bytes: usize,
) -> Result<ModelRouteRankingWireOutcome> {
    request.validate()?;
    let response = value(raw, maximum_bytes)?;
    let root = response.as_object().ok_or(Error::InvalidArguments)?;
    if root
        .keys()
        .any(|k| !["model", "answers", "usage"].contains(&k.as_str()))
        || root.get("model").and_then(Value::as_str) != Some(request.binding.adviser_model.as_str())
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
    let mut expected = (0..request.eligible_routes.len())
        .map(|i| format!("R{i}"))
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
    let mut ranked = request
        .eligible_routes
        .iter()
        .enumerate()
        .map(|(i, r)| (r.id.clone(), probabilities[&format!("R{i}")]))
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let outcome = if selected == ABSTAIN
        || probabilities
            .values()
            .filter(|p| *p == selected_probability)
            .count()
            > 1
        || ranked.windows(2).any(|w| w[0].1 == w[1].1)
    {
        ModelRouteRankingWireOutcome::Abstained {
            reason: ModelRouteWireAbstainReason::NoPreference,
        }
    } else {
        ModelRouteRankingWireOutcome::Ranked {
            route_ids: ranked.into_iter().map(|(id, _)| id).collect(),
        }
    };
    validate_model_route_ranking_outcome(request, &outcome)?;
    Ok(outcome)
}

pub(super) fn usage(raw: &[u8], maximum_bytes: usize) -> Result<ModelRouteUsage> {
    let value = value(raw, maximum_bytes)?;
    let root = value.as_object().ok_or(Error::InvalidArguments)?;
    let Some(usage) = root.get("usage") else {
        return Ok(ModelRouteUsage {
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
    Ok(ModelRouteUsage {
        input_tokens: counter("input_tokens")?,
        output_tokens: counter("output_tokens")?,
    })
}
