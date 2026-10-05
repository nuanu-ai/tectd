//! Agent-facing reply budget: each large body is delivered once per read.
//!
//! Mutation replies return identities, revisions and digests of bodies the agent
//! already received from the matching begin/open/context read; explicit reads keep
//! the bodies. The canonical records and their digests are unchanged.

use serde_json::{Value, json};

/// Program fields the agent itself just saved.
#[cfg(test)]
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

/// Compact snapshot references for every kind/version and every lifecycle reply.
/// All full fields remain available through pinned snapshot/phase/details reads.
pub(crate) fn compact_pipeline(context: &mut Value) {
    let run = context["run"].clone();
    let definition = &context["definition"];
    let count = |field: &str| context[field].as_array().map_or(0, Vec::len);
    let mut compact_run = run.clone();
    if let Some(object) = compact_run.as_object_mut() {
        object.remove("qualification_reason");
    }
    *context = json!({"run":compact_run,
        "definition":{"kind":definition["kind"],"version":definition["version"],"digest":definition["digest"]},
        "source_checkpoint":context["source_checkpoint"],"delivery_receipt":context["delivery_receipt"],
        "delivery_scope":"snapshot_reference",
        "counts":{"phases":context["definition"]["phases"].as_array().map_or(0,Vec::len),"inputs":count("inputs"),"outputs":count("outputs"),
            "bindings":count("bindings"),"attempts":count("attempts"),"checkpoints":count("checkpoints")},
        "output_availability":{"complete":context["outputs_complete"],"view":"details","section":"outputs"},
        "field_destinations":{"definition":{"view":"snapshot"},"current_phase":{"view":"phase_contract"},"delivered_phases":{"view":"details","section":"history"},
            "inputs_and_consumption_refs":{"view":"details","section":"inputs"},"knowledge_and_resource_metadata":{"view":"details","section":"inputs"},
            "bindings_and_outputs":{"view":"details","section":"outputs"},"attempts_checkpoints_inquiry_result_qualification":{"view":"details","section":"history"}}
    });
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
#[cfg(test)]
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
