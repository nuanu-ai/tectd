//! Agent-facing reply budget: each large body is delivered once per read.
//!
//! Mutation replies return identities, revisions and digests of bodies the agent
//! already received from the matching begin/open/context read; explicit reads keep
//! the bodies. The canonical records and their digests are unchanged.

use serde_json::{Map, Value, json};

/// Pipeline definition fields that a mutation reply does not repeat.
const PIPELINE_STATIC_FIELDS: [&str; 4] = [
    "completion_contract",
    "escalation_contract",
    "forbidden_claims",
    "allowed_modes",
];

/// Program fields the agent itself just saved.
const PROGRAM_SAVED_FIELDS: [&str; 6] = [
    "intent",
    "basis",
    "boundaries",
    "constraints",
    "success",
    "working_notes",
];

/// Returns the serialized pipeline run context inside a reply payload.
pub(crate) fn pipeline_context_mut(data: &mut Value) -> Option<&mut Value> {
    if data.get("definition").is_some() {
        return Some(data);
    }
    for key in ["context", "created", "replay"] {
        if data
            .get(key)
            .is_some_and(|value| value.get("definition").is_some())
        {
            return data.get_mut(key);
        }
    }
    None
}

/// Applies the reply budget to one serialized pipeline run context.
///
/// A reread keeps the static definition and prior outputs and gains a compact phase
/// map; a mutation reply keeps only the delivered phase, bindings and state.
#[cfg(test)]
pub(crate) fn pipeline_context(context: &mut Value, reread: bool, phase_map: Option<Value>) {
    pipeline_context_with_delivery(context, reread, phase_map, false, false);
}

/// Applies the normal response diet while retaining the legacy whole-delivery
/// phase list when it is the begin reply that established that delivery.
pub(crate) fn pipeline_context_with_delivery(
    context: &mut Value,
    reread: bool,
    phase_map: Option<Value>,
    preserve_delivered_phases: bool,
    preserve_outputs: bool,
) {
    let duplicate = context.get("delivered_phases").is_some()
        && !preserve_delivered_phases
        && context.get("delivered_phases") == context.pointer("/definition/phases");
    let Some(object) = context.as_object_mut() else {
        return;
    };
    if duplicate {
        object.remove("delivered_phases");
    }
    if let Some(definition) = object.get_mut("definition").and_then(Value::as_object_mut) {
        if let Some(overview) = definition
            .get_mut("overview")
            .and_then(Value::as_object_mut)
            && (!reread || is_legacy_manifest(overview))
        {
            overview.remove("body");
        }
        if reread {
            if let Some(map) = phase_map {
                definition.insert("phase_map".into(), map);
            }
        } else {
            for field in PIPELINE_STATIC_FIELDS {
                definition.remove(field);
            }
        }
    }
    if !preserve_outputs
        && object
            .get("outputs")
            .and_then(Value::as_array)
            .is_some_and(|outputs| !outputs.is_empty())
    {
        if reread {
            if let Some(outputs) = object.get_mut("outputs").and_then(Value::as_array_mut) {
                for output in outputs {
                    let Some(source) = output.as_object() else {
                        continue;
                    };
                    let stale = source.get("stale").cloned().unwrap_or(json!(false));
                    let reason = source.get("stale_reason").cloned().unwrap_or(Value::Null);
                    *output = json!({
                        "id":source.get("id"),
                        "run_id":source.get("run_id"),
                        "phase_id":source.get("phase_id"),
                        "phase_ordinal":source.get("phase_ordinal"),
                        "revision":source.get("revision"),
                        "digest":source.get("digest"),
                        "reference":source.get("reference"),
                        "target_binding":source.get("phase_id"),
                        "fresh":!stale.as_bool().unwrap_or(false),
                        "usable":!stale.as_bool().unwrap_or(false),
                        "usability_reason":if stale.as_bool() == Some(true) { reason } else { json!("current") }
                    });
                }
            }
        } else {
            object.insert("outputs".into(), json!([]));
            object.insert("outputs_complete".into(), json!(false));
        }
    }
    if !reread {
        // The receipt is backend-owned and is returned by begin/explicit
        // context refresh. Mutation replies carry only checkpoint/delta state.
        object.remove("delivery_receipt");
    }
}

/// The V1-derived pipeline overview is a serialized manifest, not agent guidance.
fn is_legacy_manifest(overview: &Map<String, Value>) -> bool {
    overview
        .get("body")
        .and_then(Value::as_str)
        .is_some_and(|body| body.trim_start().starts_with('{'))
}

/// Drops the method body copied into a planning-knowledge manifest; the planning
/// snapshot carries the same method, and the manifest keeps its identity.
pub(crate) fn planning_knowledge(value: &mut Value) {
    if let Some(method) = value
        .pointer_mut("/manifest/needs/method")
        .and_then(Value::as_object_mut)
    {
        method.remove("body");
    }
}

/// Keeps planning snapshot identities and drops method, rule and catalogue bodies
/// already delivered by the begin/open read that created the snapshot.
pub(crate) fn planning_snapshot(snapshot: &mut Value) {
    let Some(object) = snapshot.as_object_mut() else {
        return;
    };
    if let Some(method) = object.get_mut("method").and_then(Value::as_object_mut) {
        method.remove("body");
    }
    if let Some(rules) = object.get_mut("rules").and_then(Value::as_array_mut) {
        for rule in rules.iter_mut().filter_map(Value::as_object_mut) {
            rule.remove("text");
            rule.remove("applicability");
        }
    }
    if let Some(catalogue) = object.get_mut("catalogue").and_then(Value::as_object_mut) {
        catalogue.remove("entries");
    }
}

/// Applies the planning budget to a candidate context object: the manifest method
/// copy always goes; snapshot bodies go when the reply follows their delivery.
pub(crate) fn planning_context(context: &mut Value, bodies: bool) {
    if let Some(knowledge) = context.get_mut("planning_knowledge") {
        planning_knowledge(knowledge);
    }
    if !bodies && let Some(snapshot) = context.get_mut("snapshot") {
        planning_snapshot(snapshot);
    }
}

/// Applies `planning_context` inside a created or replayed outcome.
pub(crate) fn outcome_planning(value: &mut Value, field: &str, bodies: bool) {
    for outcome in ["created", "replay"] {
        if let Some(context) = value
            .get_mut(outcome)
            .and_then(|value| value.get_mut(field))
        {
            planning_context(context, bodies);
        }
    }
}

/// A saved Program reply keeps state and identities, not the fields just submitted.
pub(crate) fn saved_program(program: &mut Value) {
    let Some(object) = program.as_object_mut() else {
        return;
    };
    for field in PROGRAM_SAVED_FIELDS {
        object.remove(field);
    }
    if let Some(knowledge) = object.get_mut("planning_knowledge") {
        planning_knowledge(knowledge);
    }
}

#[cfg(test)]
#[path = "response_diet/tests.rs"]
mod tests;
