use super::recovery_support::Mcp;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{path::Path, process::Command};
use uuid::Uuid;

pub(super) fn id(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap()).unwrap()
}

fn git(path: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}

pub(super) fn repository(path: &Path) {
    std::fs::create_dir(path).unwrap();
    git(path, &["init", "--quiet", "--initial-branch=main"]);
    git(
        path,
        &[
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
    );
}

pub(super) fn planning_ref(context: &Value, sequence: i64) -> Uuid {
    context["snapshot"]["source_refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["kind"] == "planning_input" && source["input_sequence"] == sequence)
        .map(|source| id(&source["id"]))
        .unwrap()
}

pub(super) fn goal(local: &str, text: &str, source: Uuid, candidate: &str) -> Value {
    json!({
        "identity":{"local":local},"text":text,"source_ref_id":source,
        "resolution":{"kind":"candidate","reference":{"local":candidate}}
    })
}

pub(super) fn candidate(local: &str, title: &str, behavior: &str, goal: &str) -> Value {
    json!({
        "identity":{"local":local},"title":title,"outcome":behavior,
        "trigger":"Open the requested product surface","delivered_behavior":behavior,
        "proof":"Exercise the working result through existing product tests",
        "includes":[behavior],"excludes":["Unrequested adjacent work"],
        "dependencies":[],"coverage_goals":[{"local":goal}],"evidence":[]
    })
}

pub(super) fn existing_candidate(value: &Value, rationale: Option<&str>) -> Value {
    let mut result = json!({
        "identity":{"id":value["id"],"revision":value["revision"]},
        "title":value["title"],"outcome":value["outcome"],"trigger":value["trigger"],
        "delivered_behavior":value["delivered_behavior"],"proof":value["proof"],
        "includes":value["includes"],"excludes":value["excludes"],
        "dependencies":value["dependencies"],"coverage_goals":value["coverage_goal_ids"]
            .as_array().unwrap().iter().map(|id| json!({"id":id})).collect::<Vec<_>>(),
        "evidence":value["evidence_ids"].as_array().unwrap().iter()
            .map(|id| json!({"id":id})).collect::<Vec<_>>()
    });
    if let Some(rationale) = rationale {
        result["change_rationale"] = json!(rationale);
    }
    result
}

pub(super) fn existing_goal(value: &Value, source: Uuid) -> Value {
    json!({
        "identity":{"id":value["id"],"revision":value["revision"]},
        "text":value["text"],"source_ref_id":source,
        "resolution":{"kind":"candidate","reference":{"id":value["resolution"]["id"]}}
    })
}

pub(super) async fn read_text(
    client: &mut Mcp,
    set: Uuid,
    source: Uuid,
    draft_revision: Option<i64>,
) -> String {
    let mut cursor = 0_u64;
    let mut text = String::new();
    loop {
        let mut params = json!({
            "candidate_set_id":set,"view":"fragment","source_ref_id":source,"cursor":cursor
        });
        if let Some(revision) = draft_revision {
            params["draft_revision"] = json!(revision);
        }
        let page = client.call("candidate_context", params).await;
        text.push_str(page["fragment"]["text"].as_str().unwrap());
        let Some(next) = page["fragment"]["next_cursor"].as_u64() else {
            break;
        };
        assert!(next > cursor);
        cursor = next;
    }
    text
}

pub(super) async fn versions(pool: &PgPool, set: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT table_name||':'||row_value FROM (
           SELECT 'sets' table_name,xmin::text||':'||row_to_json(s)::text row_value FROM scope_candidate_sets s WHERE id=$1
           UNION ALL SELECT 'inputs',xmin::text||':'||row_to_json(i)::text FROM scope_candidate_inputs i WHERE candidate_set_id=$1
           UNION ALL SELECT 'snapshots',xmin::text||':'||row_to_json(n)::text FROM scope_candidate_snapshots n WHERE candidate_set_id=$1
           UNION ALL SELECT 'refs',xmin::text||':'||row_to_json(r)::text FROM scope_candidate_source_refs r WHERE candidate_set_id=$1
           UNION ALL SELECT 'drafts',xmin::text||':'||row_to_json(d)::text FROM scope_candidate_drafts d WHERE candidate_set_id=$1
           UNION ALL SELECT 'reviews',xmin::text||':'||row_to_json(v)::text FROM scope_candidate_reviews v WHERE candidate_set_id=$1
           UNION ALL SELECT 'receipts',xmin::text||':'||row_to_json(x)::text FROM scope_candidate_receipts x WHERE candidate_set_id=$1
         ) q ORDER BY table_name,row_value",
    )
    .bind(set)
    .fetch_all(pool)
    .await
    .unwrap()
}
