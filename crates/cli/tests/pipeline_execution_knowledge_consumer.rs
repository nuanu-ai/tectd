#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[path = "pipeline_execution/knowledge_operation_support.rs"]
#[allow(dead_code)]
mod knowledge_operation_support;
#[path = "pipeline_execution/full_support.rs"]
#[allow(dead_code)]
mod pipeline_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::commit_create;
use knowledge_operation_support::{SingleOperation, commit_single};
use pipeline_support::{completion, successful_route};
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{open_slice, ready_source_candidate, repository, review, route, route_error, save};
use tect_postgres::admin;
use uuid::Uuid;

fn pipeline_draft(label: &str) -> Value {
    json!({"coverage_summary":"Generic knowledge consumer fixture","nodes":[{
        "kind":"work","identity":{"local":label},"title":"Consume typed knowledge",
        "outcome":"The phase receives its exact applicable typed knowledge resources",
        "includes":["typed knowledge"],"excludes":["implicit current repin"],
        "dependencies":[],"proof":["Exact captured manifest"],
        "pipeline":"slice.custom-procedure-capture",
        "pipeline_reason":"Exercise generic knowledge consumption","source_result_ids":[]}],
        "supersessions":[]})
}

async fn begin(client: &mut Mcp, repo: &std::path::Path, label: &str) -> Value {
    let (source, candidate) = ready_source_candidate(client, repo).await;
    let scope=route(client,"command","scope.open",json!({"request_id":Uuid::new_v4(),
        "candidate_set_id":source["candidate_set"]["id"],"candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],
        "candidate_revision":candidate["revision"]})).await;
    let saved = save(client, &scope["created"]["planning"], pipeline_draft(label)).await;
    let reviewed = review(client, &saved).await;
    let opened = route(
        client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    route(
        client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),
        "scope_id":reviewed["scope"]["id"],"slice_id":opened["created"]["id"],
        "slice_revision":opened["created"]["revision"],"delivery_mode":"phasewise",
        "qualification_reason":"Exact generic consumer integration fixture."}),
    )
    .await["created"]
        .clone()
}

fn runbook(purpose: &str, source: &str) -> Value {
    let mut fixture: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/runbook.json"
    ))
    .unwrap();
    fixture["document"]["bindings"][0]["purpose"] = json!(purpose);
    fixture["document"]["sources"][0]["snapshot"]["uri"] = json!(format!("urn:{source}"));
    fixture["document"]["sources"][0]["snapshot"]["text"] = json!(source);
    fixture["document"].clone()
}

fn contains_unit(values: &Value, unit: &Value) -> bool {
    values.as_array().is_some_and(|values| {
        values
            .iter()
            .any(|value| value == unit || value["unit_id"] == *unit)
    })
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
    let workspace: Uuid =
        sqlx::query_scalar("SELECT id FROM workspaces WHERE tenant_id=$1 AND key=$2")
            .bind(tenant)
            .bind(workspace_key)
            .fetch_one(pool)
            .await
            .unwrap();
    let config = root.join("member-host.json");
    host_file(&config, &enrollment.auth);
    let mut client = Mcp::start(socket, &config, &Uuid::new_v4().to_string(), workspace_key).await;
    client.call("open_workspace", json!({})).await;
    (client, enrollment.principal_id, workspace)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn generic_consumer_enforces_time_pin_withdrawal_and_member_access() {
    if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("generic-focused.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("dk2-generic-focused-{}", Uuid::new_v4()),
    );
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("owner-host.json");
    host_file(&config, &enrollment.auth);
    let workspace_key = format!("generic-focused-{}", Uuid::new_v4());
    let mut owner = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    owner.call("open_workspace", json!({})).await;

    let empty = begin(&mut owner, &repo, "empty").await;
    assert!(
        empty["knowledge_resources"]["selected"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let required = commit_create(&mut owner, runbook("procedure", "required-current")).await;
    let required_unit = required.receipt["applied_operations"][0]["unit_id"].clone();
    let stale = route(
        &mut owner,
        "query",
        "slice.pipeline.context",
        json!({"run_id":empty["run"]["id"]}),
    )
    .await;
    assert_eq!(stale["knowledge_resource_status"]["state"], "stale");
    assert!(contains_unit(
        &stale["knowledge_resource_status"]["changed_unit_ids"],
        &required_unit
    ));

    let mut pinned_doc = runbook("proof_basis", "pinned-old");
    pinned_doc["bindings"][0]["version_resolution"] =
        json!({"kind":"pinned_revision","revision":1});
    let pinned = commit_create(&mut owner, pinned_doc.clone()).await;
    let pinned_unit = pinned.receipt["applied_operations"][0]["unit_id"].clone();
    let mut revised = pinned_doc.clone();
    revised["title"] = json!("pinned revision two must not replace revision one");
    let revised_sources = revised["sources"].clone();
    commit_single(
        &mut owner,
        SingleOperation {
            operation: "revise",
            unit_id: Some(pinned_unit.clone()),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
            document: Some(revised),
            revalidation: None,
            successor: None,
            replacement_bindings: json!([]),
            sources: revised_sources,
            knowledge_kind: json!("procedure"),
            profiles: json!(["general", "runbook"]),
            erasure: "not_required",
            authored_followup: false,
        },
    )
    .await;

    let mut optional = runbook("reference", "optional-expired");
    optional["valid_until"] = json!("2020-01-01T00:00:00Z");
    let optional = commit_create(&mut owner, optional).await;
    let optional_unit = optional.receipt["applied_operations"][0]["unit_id"].clone();
    let current = begin(&mut owner, &repo, "pin-and-optional").await;
    let resources = &current["knowledge_resources"];
    let selected = resources["selected"].as_array().unwrap();
    let pinned_selected = selected
        .iter()
        .find(|v| v["unit_id"] == pinned_unit)
        .unwrap();
    assert_eq!(pinned_selected["revision"], 1);
    assert!(
        resources["freshness_warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "optional_resource_expired")
    );
    assert!(!contains_unit(&resources["selected"], &optional_unit));
    let (verdict, outcome, transition) = successful_route(&current);
    let mut acknowledged = completion(&current, verdict, outcome, transition, None, None);
    acknowledged["consumed_knowledge"] =
        json!({"manifest_id":resources["id"],"digest":resources["digest"]});
    assert!(
        route(
            &mut owner,
            "command",
            "slice.pipeline.phase.complete",
            acknowledged
        )
        .await
        .get("context")
        .is_some()
    );

    let mut restricted = runbook("reference", "private-erased-marker");
    restricted["access_scope"] = json!("owners_only");
    let restricted = commit_create(&mut owner, restricted).await;
    let restricted_unit = restricted.receipt["applied_operations"][0]["unit_id"].clone();
    let owner_cached = begin(&mut owner, &repo, "owner-private-cache").await;
    assert!(contains_unit(
        &owner_cached["knowledge_resources"]["selected"],
        &restricted_unit
    ));
    let (mut member, member_principal, workspace) =
        member_client(&pool, enrollment.tenant_id, &workspace_key, &root, &socket).await;
    let allowed = route(
        &mut member,
        "query",
        "slice.pipeline.context",
        json!({"run_id":owner_cached["run"]["id"]}),
    )
    .await;
    assert!(contains_unit(
        &allowed["knowledge_resources"]["selected"],
        &restricted_unit
    ));
    sqlx::query(
        "DELETE FROM memberships WHERE tenant_id=$1 AND workspace_id=$2 AND principal_id=$3",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace)
    .bind(member_principal)
    .execute(&pool)
    .await
    .unwrap();
    let denied = route_error(
        &mut member,
        "query",
        "slice.pipeline.context",
        json!({"run_id":owner_cached["run"]["id"]}),
    )
    .await;
    assert_eq!(denied["error"]["code"], "forbidden");
    assert!(!denied.to_string().contains("private-erased-marker"));
    sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
        .bind(enrollment.tenant_id)
        .bind(workspace)
        .bind(member_principal)
        .execute(&pool)
        .await
        .unwrap();

    commit_single(
        &mut owner,
        SingleOperation {
            operation: "retract",
            unit_id: Some(required_unit.clone()),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
            document: None,
            revalidation: None,
            successor: None,
            replacement_bindings: json!([]),
            sources: json!([]),
            knowledge_kind: json!("procedure"),
            profiles: json!(["general", "runbook"]),
            erasure: "not_required",
            authored_followup: false,
        },
    )
    .await;
    let withdrawn = begin(&mut owner, &repo, "withdrawn-required").await;
    assert_eq!(
        withdrawn["knowledge_resources"]["unresolved_needs"],
        json!(["resource_unavailable"])
    );
    let (verdict, outcome, transition) = successful_route(&withdrawn);
    let mut blocked = completion(&withdrawn, verdict, outcome, transition, None, None);
    blocked["consumed_knowledge"] = json!({"manifest_id":withdrawn["knowledge_resources"]["id"],
        "digest":withdrawn["knowledge_resources"]["digest"]});
    assert_eq!(
        route_error(
            &mut owner,
            "command",
            "slice.pipeline.phase.complete",
            blocked
        )
        .await["error"]["code"],
        "needs_context"
    );

    commit_single(
        &mut owner,
        SingleOperation {
            operation: "erase",
            unit_id: Some(restricted_unit),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
            document: None,
            revalidation: None,
            successor: None,
            replacement_bindings: json!([]),
            sources: json!([]),
            knowledge_kind: json!("procedure"),
            profiles: json!(["general", "runbook"]),
            erasure: "owned_live_copies",
            authored_followup: false,
        },
    )
    .await;
    let erased = route_error(
        &mut owner,
        "query",
        "slice.pipeline.context",
        json!({"run_id":owner_cached["run"]["id"]}),
    )
    .await;
    assert_eq!(erased["error"]["code"], "knowledge_payload_erased");
    assert!(!erased.to_string().contains("private-erased-marker"));
    member.finish().await;
    owner.finish().await;
}
