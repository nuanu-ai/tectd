//! What the `model` tests do beyond the shared fixtures: the rule the loopback Jev
//! answers both points of an input by, the fields its answers make, and the two
//! journal rows of one recorded input.
use crate::drafts::Level;
use crate::recovery_support::{Mcp, tool_payload};
use crate::support::Stand;
use serde_json::{Value, json};
use tect_domain::{INTAKE_NOTE, MODEL_NOTE};
use uuid::Uuid;

pub use crate::pipeline::{enable, points, rows};

pub const NOT_AN_ANSWER: &str = "Jev advice is not available: the answer has no `answers` object";

/// `intake` finds bounded new work that needs a candidate of its own and is in line;
/// `model` finds work for the implementation executor and none for a specialist.
pub fn settled(name: &str) -> (&'static str, f64) {
    match name {
        "work" => ("bounded", 0.7),
        "placement" => ("new_candidate", 0.7),
        "nearest" => ("none", 0.7),
        "fit" => ("in_line", 0.7),
        "practical" => ("implementation", 0.7149),
        "specialist" => ("none", 0.875),
        other => panic!("unexpected question {other}"),
    }
}

/// The `model_advice` field for `settled`.
pub fn advised() -> Value {
    json!({"status":"advised","note":MODEL_NOTE,
        "practical":"implementation","practical_p":0.71,
        "specialist":"none","specialist_p":0.88})
}

/// The `advice` field of `intake` for `settled`. `nearest` is asked when the set has
/// a saved draft with a candidate to name.
pub fn intake_advised(level: Level, drafted: bool) -> Value {
    let mut field = json!({"status":"advised","note":INTAKE_NOTE,"level":level.name(),
        "work":"bounded","work_p":0.7,"placement":"new_candidate","placement_p":0.7,
        "fit":"in_line","fit_p":0.7});
    if drafted {
        field["nearest"] = json!("none");
        field["nearest_p"] = json!(0.7);
    }
    field
}

/// A field of either point that says it has no advice for this reason.
pub fn unavailable(reason: &str, message: &str) -> Value {
    json!({"status":"unavailable","reason":reason,
        "message":format!("Jev advice is not available: {message}")})
}

/// The two rows one recorded input wrote: that of `intake`, then that of `model`.
pub async fn both_rows(stand: &Stand, request_id: Uuid) -> (Value, Value) {
    let written = rows(stand, request_id).await;
    assert_eq!(points(&written), ["intake", "model"]);
    let [intake, model]: [Value; 2] = written.try_into().unwrap();
    (intake, model)
}

/// What the journal says of a row: whether Jev was called, how certain the send is,
/// the outcome and the reason.
pub fn journalled(row: &Value) -> (&Value, &Value, &Value, &Value) {
    (
        &row["called"],
        &row["send_certainty"],
        &row["outcome"],
        &row["reason"],
    )
}

/// The `model` rows of the journal through the audit route, newest first, without bodies.
pub async fn audit(client: &mut Mcp) -> Value {
    let response = client
        .exchange(
            "tools/call",
            json!({"name":"query","arguments":{"route":"workspace.advisory.audit",
                "params":{"limit":10,"point":"model","bodies":false}}}),
        )
        .await;
    tool_payload(&response)
}
