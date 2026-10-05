use super::*;
use serde_json::{Value, json};

fn report() -> Value {
    json!({"stage":"specification", "rules_digest":"rules", "verdict":"pass",
        "reviewed_outputs":[], "source_basis":"source", "prior_finding_ids":[],
        "resolved_finding_ids":[], "assessments": RULES.iter().map(|rule| json!({
            "rule_id": rule, "status":"satisfied", "rationale":"basis", "evidence_refs":["source"]
        })).collect::<Vec<_>>(), "findings":[], "files":[file()], "summary":"checked"})
}
fn file() -> Value {
    json!({"path":"src/main.rs", "content_kind":"behavioral", "line_count":100,
    "count_basis":"observed", "content_digest":"a".repeat(64), "responsibility":"main", "justification":null})
}
fn finding() -> Value {
    json!({"id":"F1", "rule_id":"ENG-01", "status":"resolved", "evidence":"source", "resolution":"fixed"})
}
fn request() -> CompletePipelinePhase {
    serde_json::from_value(json!({
    "request_id":uuid::Uuid::nil(), "run_id":uuid::Uuid::nil(), "run_revision":1, "phase_id":"review",
    "outcome":"completed", "transition":"continue", "output":{"producer_context_id":"ctx", "verdict":"PASS"}
})).unwrap()
}
fn check(value: Value, request: &CompletePipelinePhase, stage: &str, lineage: bool) -> Result<()> {
    validate_report(
        &serde_json::from_value(value).unwrap(),
        request,
        2,
        stage,
        "rules",
        &["PASS".into()],
        lineage,
    )
}
fn assert_rule(error: Error, rule: &str, pointer: &str) {
    let refusal = error.refusal().unwrap();
    assert_eq!(refusal.code, RefusalCode::InvalidOutput);
    assert_eq!(refusal.rule.as_deref(), Some(rule));
    assert_eq!(refusal.path.as_deref(), Some("output.artifacts[2].body"));
    assert!(
        refusal
            .expected
            .as_deref()
            .unwrap()
            .contains(&format!("decoded JSON {pointer}:"))
    );
    assert_eq!(refusal.next_action.as_deref(), Some("correct_output"));
}

mod constraints;
mod report;
