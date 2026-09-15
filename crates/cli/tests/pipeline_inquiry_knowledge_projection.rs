#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[path = "setup_capacity/legacy.rs"]
mod legacy_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::commit_create;
use legacy_support::{LegacyDaemon, LegacyMcp};
use recovery_support::{
    Daemon, Mcp, action_params, find_action, host_file, private_temp, tagged_url,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use support::{open_slice, ready_source_candidate, repository, review, route, save};
use tect_postgres::admin;
use uuid::Uuid;

const DETAIL_MARKER: &str = "DETAIL-CANONICAL-MARKER ExampleDriver 7.4.2";
const LEGACY_MARKER: &str = "LEGACY-PHASE-ONLY-MARKER";
const PROGRAM_INSTRUCTION: &str = "Plan service operation within region R1.";
const SCOPE_INSTRUCTION: &str =
    "Keep R1 delivery and regional isolation verification in one Scope.";

fn binding(target: Value, purpose: &str) -> Value {
    json!({"target":target,"purpose":purpose,
        "version_resolution":{"kind":"current_accepted"}})
}

fn document(label: &str, bindings: Value, access_scope: &str, selected: bool) -> Value {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/runbook.json"
    ))
    .unwrap();
    let document = &mut value["document"];
    document["title"] = json!(label);
    document["canonical_text"] = json!(DETAIL_MARKER);
    document["conditions"] = json!(["detailed condition must not enter high context"]);
    document["exceptions"] = json!(["detailed exception must not enter high context"]);
    document["access_scope"] = json!(access_scope);
    document["bindings"] = bindings;
    document["sources"][0]["snapshot"]["uri"] = json!(format!("urn:{label}"));
    document["sources"][0]["snapshot"]["text"] = json!(format!("source for {label}"));
    if selected {
        document["planning_briefs"] = json!([{
            "local_id":"program-r1","stage":"program","instruction":PROGRAM_INSTRUCTION,
            "conditions":["region R1 is selected"],"exceptions":["other regions remain excluded"],
            "purpose":"Bound strategic operation to R1.",
            "selectors":{"target_iris":["urn:fixture:r1"],
                "environment_iris":["urn:fixture:prod"],"action_classes":["deploy"]}
        },{
            "local_id":"scope-r1","stage":"scope","instruction":SCOPE_INSTRUCTION,
            "conditions":["region R1 is selected"],"exceptions":["other regions remain excluded"],
            "purpose":"Keep delivery and isolation proof together.",
            "selectors":{"target_iris":["urn:fixture:r1"],
                "environment_iris":["urn:fixture:prod"],"action_classes":["deploy"]}
        }]);
    } else {
        document.as_object_mut().unwrap().remove("planning_briefs");
    }
    document.clone()
}

fn inquiry(topic_level: &str, task_context: Value) -> Value {
    json!({"topic_level":topic_level,"task_context":task_context,
        "completion":{"kind":"research","allow_inconclusive":false}})
}

fn candidate_draft() -> Value {
    let nodes = ["program", "scope", "slice", "unknown", "empty", "mismatch"]
        .into_iter()
        .map(|label| {
            json!({
                "kind":"work","identity":{"local":label},"title":format!("{label} inquiry"),
                "outcome":"The exact inquiry context is delivered","includes":["inquiry"],
                "excludes":["publication"],"dependencies":[],"proof":["Exact manifest projection"],
                "pipeline":"slice.research","pipeline_reason":"Exercise frozen inquiry projection",
                "source_result_ids":[]
            })
        })
        .collect::<Vec<_>>();
    json!({"coverage_summary":"Exercise inquiry projection by topic height.",
        "nodes":nodes,"supersessions":[]})
}

async fn open_targets(
    client: &mut Mcp,
    pool: &PgPool,
    repo: &std::path::Path,
) -> (Value, Vec<Value>) {
    let (source, candidate) = ready_source_candidate(client, repo).await;
    let scope=route(client,"command","scope.open",json!({"request_id":Uuid::new_v4(),
        "candidate_set_id":source["candidate_set"]["id"],"candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],
        "candidate_revision":candidate["revision"]})).await;
    let saved = save(client, &scope["created"]["planning"], candidate_draft()).await;
    let reviewed = review(client, &saved).await;
    let scope_id = Uuid::parse_str(reviewed["scope"]["id"].as_str().unwrap()).unwrap();
    let mut slices = Vec::new();
    for node in reviewed["draft"]["nodes"].as_array().unwrap() {
        let current_revision: i64 =
            sqlx::query_scalar("SELECT revision FROM native_scopes WHERE id=$1")
                .bind(scope_id)
                .fetch_one(pool)
                .await
                .unwrap();
        let mut params = open_slice(&reviewed, node, Uuid::new_v4());
        params["scope_revision"] = json!(current_revision);
        let opened = route(client, "command", "slice.open", params).await;
        slices.push(opened["created"].clone());
    }
    (reviewed["scope"].clone(), slices)
}

async fn begin(client: &mut Mcp, scope: &Value, slice: &Value, inquiry: Value) -> Value {
    route(
        client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),"scope_id":scope["id"],"slice_id":slice["id"],
            "slice_revision":slice["revision"],"delivery_mode":"phasewise",
            "qualification_reason":"Exact inquiry projection integration fixture.","inquiry":inquiry}),
    )
    .await["created"]
        .clone()
}

fn selected_for<'a>(context: &'a Value, unit: &Value) -> Vec<&'a Value> {
    context["knowledge_resources"]["selected"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|resource| resource["unit_id"] == *unit)
        .collect()
}

fn run_legacy_admin(binary: &Path, url: &str, arguments: &[&str]) {
    assert!(
        Command::new(binary)
            .env("TECT_ADMIN_DATABASE_URL", url)
            .args(arguments)
            .status()
            .unwrap()
            .success()
    );
}

async fn legacy_route(client: &mut LegacyMcp, route_name: &str, params: Value) -> Value {
    let (ok, payload) = client
        .call("command", json!({"route":route_name,"params":params}))
        .await;
    assert!(ok, "legacy route {route_name} failed: {payload}");
    payload
}

async fn member_client(
    pool: &PgPool,
    tenant: Uuid,
    workspace_key: &str,
    root: &std::path::Path,
    socket: &std::path::Path,
) -> (Mcp, Uuid, Uuid) {
    let enrollment = admin::enroll_host(
        pool,
        Some(tenant),
        vec![root.to_string_lossy().into_owned()],
    )
    .await
    .unwrap();
    let config = root.join("member-host.json");
    host_file(&config, &enrollment.auth);
    let workspace: Uuid =
        sqlx::query_scalar("SELECT id FROM workspaces WHERE tenant_id=$1 AND key=$2")
            .bind(tenant)
            .bind(workspace_key)
            .fetch_one(pool)
            .await
            .unwrap();
    let mut client = Mcp::start(socket, &config, &Uuid::new_v4().to_string(), workspace_key).await;
    client.call("open_workspace", json!({})).await;
    (client, enrollment.principal_id, workspace)
}

fn assert_projected(resource: &Value, instruction: &str, local_id: &str) {
    assert_eq!(resource["canonical_text"], instruction);
    assert_eq!(resource["conditions"], json!(["region R1 is selected"]));
    assert_eq!(
        resource["exceptions"],
        json!(["other regions remain excluded"])
    );
    assert_eq!(resource["inquiry_briefs"].as_array().unwrap().len(), 1);
    assert_eq!(resource["inquiry_briefs"][0]["local_id"], local_id);
    assert_eq!(resource["binding"]["purpose"], "required");
    assert!(
        resource["rdf_digest"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(!resource["source_pins"].as_array().unwrap().is_empty());
    assert!(
        resource["sections"]
            .as_object()
            .unwrap()
            .values()
            .all(Value::is_null)
    );
    assert!(!resource.to_string().contains(DETAIL_MARKER));
}

#[path = "pipeline_inquiry_knowledge_projection/legacy.rs"]
mod legacy;
#[path = "pipeline_inquiry_knowledge_projection/scenario.rs"]
mod scenario;
