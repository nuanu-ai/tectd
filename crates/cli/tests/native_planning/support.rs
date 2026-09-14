use super::recovery_support::Mcp;
use serde_json::{Value, json};
use std::{path::Path, process::Command};
use uuid::Uuid;

pub(super) fn id(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap()).unwrap()
}

fn planning_guard(value: &Value) -> Option<Value> {
    let manifest = &value["planning_knowledge"]["manifest"];
    manifest["id"].as_str().map(|_| {
        json!({"manifest_id":manifest["id"],"digest":manifest["digest"],
            "workspace_generation":manifest["workspace_generation"]})
    })
}

pub(super) fn repository(path: &Path) {
    std::fs::create_dir(path).unwrap();
    for args in [
        vec!["init", "--quiet", "--initial-branch=main"],
        vec![
            "-c",
            "user.name=Tect Test",
            "-c",
            "user.email=tect@example.invalid",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    ] {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(path)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
}

#[allow(dead_code)]
pub(super) async fn route(client: &mut Mcp, tool: &str, name: &str, params: Value) -> Value {
    client
        .call(tool, json!({"route":name,"params":params}))
        .await
}

#[allow(dead_code)]
pub(super) async fn route_error(client: &mut Mcp, tool: &str, name: &str, params: Value) -> Value {
    client
        .call_error(tool, json!({"route":name,"params":params}))
        .await
}

pub(super) async fn ready_source_candidate(client: &mut Mcp, source: &Path) -> (Value, Value) {
    client.call("open_workspace", json!({})).await;
    let registered = client.call("register_source", json!({"path":source})).await;
    let worktree = registered["id"].clone();
    client
        .call("select_worktrees", json!({"worktree_ids":[worktree]}))
        .await;
    let begun = client
        .call(
            "begin_program",
            json!({
                "request_id":Uuid::new_v4(),
                "input":"Diagnose the incorrect preview, then select the smallest correction."
            }),
        )
        .await;
    let mut program_save = json!({
        "program_id":begun["program"]["id"],"revision":1,"input_cursor":1,
        "name":"Notification preview","intent":"Correct preview behavior from demonstrated evidence",
        "basis":"The preview differs from saved settings","boundaries":"Preview diagnosis and bounded correction",
        "constraints":"No deployment or adjacent notification work","success":"The cause and correction are verified",
        "complete":true
    });
    if let Some(guard) = planning_guard(&begun["program"]) {
        program_save["consumed_knowledge"] = guard;
    }
    let program = client.call("save_program", program_save).await;
    let candidates = client.call("begin_candidate_set", json!({
        "request_id":Uuid::new_v4(),"program_id":program["program"]["id"],
        "program_revision":program["program"]["revision"],"boundary":"ongoing",
        "input":"Open one native Scope for diagnosis and its result-driven correction decision."
    })).await;
    let context = &candidates["context"];
    let inputs = client
        .call(
            "candidate_context",
            json!({
                "candidate_set_id":context["candidate_set"]["id"],"view":"inputs","limit":25
            }),
        )
        .await;
    let source_ref = &inputs["items"][0]["input"]["source_ref_id"];
    let mut candidate_save = json!({
        "kind":"draft","candidate_set_id":context["candidate_set"]["id"],"revision":1,
        "snapshot_id":context["snapshot"]["id"],"input_cursor":1,"request_id":Uuid::new_v4(),
        "draft":{"boundary":"ongoing","goals":[{
            "identity":{"local":"goal"},"text":"Explain preview deviation and bound correction",
            "source_ref_id":source_ref,"resolution":{"kind":"candidate","reference":{"local":"scope"}}
        }],"evidence":[],"candidates":[{
            "identity":{"local":"scope"},"title":"Preview diagnosis and correction decision",
            "outcome":"The cause is demonstrated and the correction path selected","trigger":"Preview differs",
            "delivered_behavior":"Cause and bounded follow-up are available","proof":"Direct evidence is retained",
            "includes":["diagnosis","decision"],"excludes":["deployment"],"dependencies":[],
            "coverage_goals":[{"local":"goal"}],"evidence":[]
        }],"blockers":[],"protected_changes":[]}
    });
    if let Some(guard) = planning_guard(context) {
        candidate_save["consumed_knowledge"] = guard;
    }
    let saved = client.call("save_candidate_set", candidate_save).await;
    let candidate = saved["draft"]["candidates"][0].clone();
    let mut candidate_review = json!({
        "kind":"review","candidate_set_id":saved["context"]["candidate_set"]["id"],
        "revision":saved["context"]["candidate_set"]["revision"],"snapshot_id":context["snapshot"]["id"],
        "input_cursor":saved["context"]["candidate_set"]["input_cursor"],"request_id":Uuid::new_v4(),
        "review":{"verdict":"ready","summary":"Bounded, vertical and ready","findings":[],
            "candidate_decisions":[{"candidate_id":candidate["id"],"decision":"accept","rationale":"Coherent Scope"}]}
    });
    if let Some(guard) = planning_guard(&saved["context"]) {
        candidate_review["consumed_knowledge"] = guard;
    }
    let reviewed = client.call("save_candidate_set", candidate_review).await;
    (reviewed["context"].clone(), candidate)
}

#[allow(dead_code)]
pub(super) fn open_slice(context: &Value, candidate: &Value, request: Uuid) -> Value {
    json!({"request_id":request,"scope_id":context["scope"]["id"],
        "scope_revision":context["scope"]["revision"],"candidate_set_id":context["candidate_set"]["id"],
        "candidate_set_revision":context["candidate_set"]["revision"],
        "candidate_snapshot_id":context["snapshot"]["id"],"candidate_id":candidate["id"],
        "candidate_revision":candidate["revision"]})
}

#[allow(dead_code)]
pub(super) async fn save(client: &mut Mcp, context: &Value, draft: Value) -> Value {
    let mut params = json!({
        "kind":"draft","scope_id":context["scope"]["id"],"candidate_set_id":context["candidate_set"]["id"],
        "revision":context["candidate_set"]["revision"],"snapshot_id":context["snapshot"]["id"],
        "input_cursor":context["candidate_set"]["input_cursor"],"request_id":Uuid::new_v4(),"draft":draft
    });
    if let Some(guard) = planning_guard(context) {
        params["consumed_knowledge"] = guard;
    }
    route(client, "command", "slice.candidates.save", params).await
}

#[allow(dead_code)]
pub(super) async fn review(client: &mut Mcp, context: &Value) -> Value {
    let mut params = json!({
        "kind":"review","scope_id":context["scope"]["id"],"candidate_set_id":context["candidate_set"]["id"],
        "revision":context["candidate_set"]["revision"],"snapshot_id":context["snapshot"]["id"],
        "input_cursor":context["candidate_set"]["input_cursor"],"request_id":Uuid::new_v4(),
        "review":{"verdict":"ready","summary":"Complete and structurally valid","findings":[]}
    });
    if let Some(guard) = planning_guard(context) {
        params["consumed_knowledge"] = guard;
    }
    route(client, "command", "slice.candidates.save", params).await
}
