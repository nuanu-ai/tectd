use super::{Mcp, route};
use serde_json::{Map, Value, json};
use uuid::Uuid;

pub(super) const LIGHTWEIGHT_PHASES: [&str; 14] = [
    "slice-lightweight-entry-gate",
    "slice-lightweight-intent-capture",
    "slice-lightweight-context-loader",
    "slice-workspace-preflight-lite",
    "slice-lightweight-contract-writer",
    "slice-lightweight-escalation-checker",
    "slice-test-target-selector",
    "slice-tdd-cycle-runner",
    "slice-implementation-note-writer",
    "slice-lightweight-verification-runner",
    "slice-deploy-impact-checker",
    "slice-lightweight-result-writer",
    "slice-lightweight-promotion-router",
    "slice-lightweight-maintenance-and-handoff",
];

pub(super) fn lightweight_draft() -> Value {
    json!({"coverage_summary":"One bounded TDD implementation lifecycle","nodes":[{
        "kind":"work","identity":{"local":"lightweight"},
        "title":"Correct one bounded preview behavior",
        "outcome":"The preview behavior is covered by focused RED/GREEN proof",
        "includes":["focused behavior","local verification"],
        "excludes":["deployment","adjacent redesign"],"dependencies":[],
        "proof":["Meaningful RED then focused GREEN"],
        "pipeline":"slice.lightweight-tdd-development",
        "pipeline_reason":"The behavior and proof target are bounded",
        "source_result_ids":[]
    }],"supersessions":[]})
}

pub(super) fn phase_output(phase: &Value, marker: &str, outcome: &str, transition: &str) -> Value {
    let mut fields = phase["required_fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| {
            (
                field.as_str().unwrap().to_owned(),
                Value::String(format!("fixture evidence for {marker}")),
            )
        })
        .collect::<Map<_, _>>();
    if phase["id"] == "slice-tdd-cycle-runner" {
        for (key, value) in [
            ("red_exit_code", "1"),
            ("green_exit_code", "0"),
            ("selected_test_identity_recorded", "true"),
            ("red_command_evidence_recorded", "true"),
            ("red_failure_observed", "true"),
            ("source_change_identity_recorded", "true"),
            ("green_command_evidence_recorded", "true"),
            ("green_pass_observed", "true"),
            ("same_target_binding_verified", "true"),
        ] {
            fields.insert(key.into(), Value::String(value.into()));
        }
        fields.insert(
            "target_binding".into(),
            Value::String("fixture-focused-target".into()),
        );
        fields.insert(
            "selected_test_target".into(),
            Value::String("fixture-focused-target".into()),
        );
    }
    if phase["id"] == "slice-lightweight-verification-runner" {
        for (key, value) in [
            ("focused_exit_code", "0"),
            ("affected_exit_code", "0"),
            ("focused_proof_disposition_recorded", "true"),
            ("affected_proof_disposition_recorded", "true"),
            ("command_evidence_or_blocker_recorded", "true"),
            ("proof_target_binding_or_gap_recorded", "true"),
            ("verification_receipt_complete", "true"),
        ] {
            fields.insert(key.into(), Value::String(value.into()));
        }
    }
    if phase["id"] == "slice-deploy-impact-checker" {
        fields.insert(
            "deploy_impact_decision".into(),
            Value::String("no_deploy_required".into()),
        );
    }
    let skills = phase["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|skill| {
            json!({"instruction_id":skill["id"],"version":skill["version"],"digest":skill["digest"]})
        })
        .collect::<Vec<_>>();
    let route = phase["verdict_routes"].as_array().and_then(|routes| {
        routes
            .iter()
            .find(|route| route["outcome"] == outcome && route["transition"] == transition)
    });
    let dispositions = route
        .map(|route| route["dispositions"].clone())
        .unwrap_or_else(|| phase["required_dispositions"].clone());
    let mut output = json!({
        "body":format!("Deterministic integration evidence for {marker}. This checks the native contract and does not claim independent semantic review."),
        "producer_context_id":format!("fixture-producer:{marker}"),
        "fields":fields,
        "dispositions":dispositions,
        "skill_reads":skills,
        "reference":format!("fixture/{marker}.md")
    });
    if let Some(verdict) = route
        .map(|route| &route["verdict"])
        .or_else(|| phase["allowed_verdicts"].as_array().unwrap().first())
    {
        output["verdict"] = verdict.clone();
    }
    output
}

pub(super) fn terminal(summary: &str) -> Value {
    json!({
        "summary":summary,
        "evidence":[{"kind":"integration_test","reference":"pipeline_execution.rs",
            "observation":"The caller reports the structural phase contract completed."}],
        "scope_impact":"The managed Result becomes one new planning input.",
        "remaining_work":"Refresh the affected future Slice graph."
    })
}

pub(super) fn consumed_outputs(context: &Value) -> Value {
    let current_ordinal = context["run"]["current_phase_ordinal"].as_u64().unwrap();
    Value::Array(
        context["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|binding| {
                binding["stale"] == false
                    && binding["phase_ordinal"].as_u64().unwrap() < current_ordinal
            })
            .map(|binding| {
                let output = context["outputs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|output| {
                        output["phase_id"] == binding["phase_id"]
                            && output["revision"] == binding["output_revision"]
                            && output["stale"] == false
                    })
                    .expect("a current binding exposes its pinned output body");
                assert_eq!(output["digest"], binding["output_digest"]);
                json!({
                    "phase_id":binding["phase_id"],
                    "output_revision":binding["output_revision"],
                    "digest":output["digest"]
                })
            })
            .collect(),
    )
}

pub(super) fn consumed_inputs(context: &Value) -> Value {
    let phase_id = &context["run"]["current_phase_id"];
    Value::Array(
        context["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|input| input["phase_id"] == *phase_id)
            .map(|input| {
                json!({"input_id":input["id"],"sequence":input["sequence"],"digest":input["digest"]})
            })
            .collect(),
    )
}

pub(super) async fn complete(
    client: &mut Mcp,
    context: &Value,
    outcome: &str,
    transition: &str,
    terminal_result: Option<Value>,
    publish_blocked_result: bool,
) -> (Value, Value) {
    let phase_id = context["run"]["current_phase_id"].as_str().unwrap();
    let phase = context["definition"]["phases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|phase| phase["id"] == phase_id)
        .unwrap();
    let request_id = Uuid::new_v4();
    let mut params = json!({
        "request_id":request_id,"run_id":context["run"]["id"],
        "run_revision":context["run"]["revision"],"phase_id":phase_id,
        "outcome":outcome,"transition":transition,
        "output":phase_output(phase,phase_id,outcome,transition),
        "consumed_outputs":consumed_outputs(context),
        "consumed_inputs":consumed_inputs(context),
        "publish_blocked_result":publish_blocked_result
    });
    if let Some(result) = terminal_result {
        params["terminal_result"] = result;
    }
    let response = route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        params.clone(),
    )
    .await;
    (response, params)
}
