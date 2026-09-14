#[path = "knowledge_lifecycle/canonical_operations.rs"]
mod canonical_operations;
#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[path = "pipeline_execution/knowledge_operation_support.rs"]
mod knowledge_operation_support;
#[path = "knowledge_lifecycle/planning_delivery.rs"]
mod planning_delivery;
#[path = "knowledge_lifecycle/planning_pins.rs"]
mod planning_pins;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::{
    commit_create, complete_agent, context, method_reads, omit_nulls, settle_and_finish,
    settle_and_finish_receipt,
};
use knowledge_operation_support::{
    SingleOperation, commit_pair_erase, commit_single, ready_single_from_baseline_with_reviewed,
};
use recovery_support::{Daemon, Mcp, action_params, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{repository, route, route_error};
use tect_postgres::admin;
use uuid::Uuid;

fn planning_guard(manifest: &Value) -> Value {
    json!({
        "manifest_id": manifest["id"],
        "digest": manifest["digest"],
        "workspace_generation": manifest["workspace_generation"]
    })
}

async fn ready_program(client: &mut Mcp, label: &str) -> Value {
    let begun = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":format!("Form {label}.")}),
        )
        .await;
    let saved = client
        .call(
            "save_program",
            json!({"program_id":begun["program"]["id"],"revision":1,"input_cursor":1,
                "name":label,"intent":"Exercise exact pinned planning knowledge",
                "basis":"A reviewed maintenance revision fixture",
                "boundaries":"One exact Program binding","constraints":"No implicit version move",
                "success":"Pinned and current revisions retain distinct status","complete":true}),
        )
        .await;
    saved["program"].clone()
}

async fn refresh_program(client: &mut Mcp, program: &Value, target: &str) -> Value {
    let refreshed = route(
        client,
        "command",
        "program.knowledge.refresh",
        json!({"program_id":program["id"],"revision":program["revision"],
            "input_cursor":program["input_cursor"],"request_id":Uuid::new_v4(),
            "task_context":{"target_iris":[target]}}),
    )
    .await;
    refreshed["program"].clone()
}

fn fixture_task_context() -> Value {
    json!({
        "target_iris":["urn:tect:dk4:fixture:unrelated","urn:tect:dk4:fixture:service-operation"],
        "environment_iris":["urn:tect:dk4:fixture:region:R2","urn:tect:dk4:fixture:region:R1"],
        "action_classes":["verify","deploy"]
    })
}

fn assert_planning_abstraction(value: &Value, expected: &str) {
    let selected = value["selected"].as_array().unwrap();
    assert_eq!(selected.len(), 1, "{value}");
    assert_eq!(selected[0]["instruction"], expected);
    let delivered = serde_json::to_string(selected).unwrap();
    assert!(!delivered.contains("ExampleDriver 7.4.2-fixture"));
    assert!(!delivered.contains("examplectl deploy --region=R1"));
    assert!(!delivered.contains("Service operation is permitted only"));
}

fn planning_document(target: &str, purposes: &[&str]) -> Value {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/planning-abstraction.json"
    ))
    .unwrap();
    value["document"]["title"] = json!(format!("Planning purpose fixture {target}"));
    value["document"]["canonical_text"] =
        json!(format!("Reviewed planning boundary for {target}."));
    value["document"]["target_iris"] = json!([target]);
    value["document"]["sources"][0]["snapshot"]["uri"] = json!(format!("{target}:source"));
    value["document"]["sources"][0]["snapshot"]["text"] =
        json!(format!("Exact source for {target}."));
    value["document"]["bindings"] = Value::Array(
        purposes
            .iter()
            .map(|purpose| {
                json!({"target":{"kind":"workspace"},"purpose":purpose,
                    "version_resolution":{"kind":"current_accepted"}})
            })
            .collect(),
    );
    value["document"]["planning_briefs"] = json!([{
        "local_id":"program-purpose","stage":"program",
        "instruction":format!("Apply the reviewed planning boundary for {target}."),
        "conditions":[],"exceptions":[],"purpose":"Planning purpose regression.",
        "selectors":{"target_iris":[target]}
    }]);
    value["document"].clone()
}

async fn mark_needs_review(pool: &PgPool, unit: &Value) -> (Uuid, String) {
    let unit = Uuid::parse_str(unit.as_str().unwrap()).unwrap();
    let (tenant, workspace, principal, session): (Uuid, Uuid, Uuid, Uuid) = sqlx::query_as(
        "SELECT tenant_id,workspace_id,actor_principal_id,actor_session_id \
         FROM knowledge_publication_events WHERE unit_id=$1 AND unit_revision=1 \
         ORDER BY created_at DESC,id DESC LIMIT 1",
    )
    .bind(unit)
    .fetch_one(pool)
    .await
    .unwrap();
    let signal = Uuid::new_v4();
    let task = Uuid::new_v4();
    let basis = json!({"kind":"operator_requested","subject_ref":format!("urn:review:{unit}"),
        "observation_digest":format!("review-{unit}")});
    let digest = format!("{:0<64}", unit.simple());
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO knowledge_maintenance_signals \
         (id,tenant_id,workspace_id,request_id,unit_id,unit_revision,reason,basis,basis_digest, \
          actor_principal_id,actor_session_id) VALUES ($1,$2,$3,$4,$5,1,'operator_requested',$6,$7,$8,$9)",
    )
    .bind(signal)
    .bind(tenant)
    .bind(workspace)
    .bind(Uuid::new_v4())
    .bind(unit)
    .bind(basis)
    .bind(&digest)
    .bind(principal)
    .bind(session)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO knowledge_maintenance_tasks (id,tenant_id,workspace_id,signal_id) \
         VALUES ($1,$2,$3,$4)",
    )
    .bind(task)
    .bind(tenant)
    .bind(workspace)
    .bind(signal)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE workspace_knowledge_state SET generation=generation+1 \
         WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(tenant)
    .bind(workspace)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    (task, digest)
}

#[tokio::test]
async fn dk2_identity_qualification_is_exact_and_repeatable() {
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
    let first = tect_postgres::current_knowledge_database_identity(&pool)
        .await
        .unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();
    let stored: (String, i64, bool) = sqlx::query_as(
        "SELECT qualified_system_identifier,qualified_database_oid::bigint, \
         tect_dk_database_identity_ready() FROM durable_knowledge_capability WHERE singleton",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored.0, first.system_identifier);
    assert_eq!(stored.1, i64::from(first.database_oid));
    assert!(stored.2);
    let runtime = PgPool::connect(&runtime_url).await.unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT tect_dk_database_identity_ready()")
            .fetch_one(&runtime)
            .await
            .unwrap()
    );
    let internal_execute: bool = sqlx::query_scalar(
        "SELECT pg_catalog.has_function_privilege($1, \
         'public.tect_dk2_internal_native_read(uuid,uuid,uuid,bigint,uuid,boolean)', \
         'EXECUTE')",
    )
    .bind(&role)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!internal_execute);
}

#[tokio::test]
async fn dk2_create_reaches_native_exact_read_and_terminal_result() {
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
    let socket = root.join("dk2.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-dk2-{}", Uuid::new_v4()));
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let mut client = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &format!("dk2-{}", Uuid::new_v4()),
    )
    .await;
    client.call("open_workspace", json!({})).await;

    let fixture: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let committed = commit_create(&mut client, fixture["document"].clone()).await;
    assert_eq!(committed.exact["document"]["document"], fixture["document"]);
    let finished = settle_and_finish(&mut client, &committed).await;
    assert_eq!(context(&finished)["run"]["status"], "completed");
    client.finish().await;
}
