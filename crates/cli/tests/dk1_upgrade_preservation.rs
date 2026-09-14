#[path = "setup_capacity/legacy.rs"]
mod legacy;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use legacy::{LegacyDaemon, LegacyMcp};
use recovery_support::{Daemon, Mcp, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::path::Path;
use std::process::Command;
use std::str::FromStr;
use support::{route, route_error};
use tect_postgres::admin;
use uuid::Uuid;

fn database_url(url: &str, database: &str) -> String {
    let (base, query) = url
        .split_once('?')
        .map_or((url, None), |(base, query)| (base, Some(query)));
    let slash = base
        .rfind('/')
        .expect("PostgreSQL URL must contain database path");
    format!(
        "{}/{}{}",
        &base[..slash],
        database,
        query.map_or(String::new(), |value| format!("?{value}"))
    )
}

fn quoted_database(name: &str) -> String {
    assert!(
        name.bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    format!("\"{name}\"")
}

async fn connect_database(url: &str, database: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(2)
        .connect_with(PgConnectOptions::from_str(url).unwrap().database(database))
        .await
        .unwrap()
}

fn run_admin(binary: &Path, url: &str, arguments: &[&str]) {
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

async fn legacy_query(client: &mut LegacyMcp, route_name: &str, params: Value) -> Value {
    let (ok, payload) = client
        .call("query", json!({"route":route_name,"params":params}))
        .await;
    assert!(ok, "legacy query {route_name} failed: {payload}");
    payload
}

async fn legacy_route_error(client: &mut LegacyMcp, route_name: &str, params: Value) -> Value {
    let (ok, payload) = client
        .call("command", json!({"route":route_name,"params":params}))
        .await;
    assert!(
        !ok,
        "legacy route {route_name} unexpectedly succeeded: {payload}"
    );
    payload
}

fn legacy_draft(title: &str, target: &str, source_text: &str) -> Value {
    json!({"title":title,"statement":format!("Retain the exact approved {title}."),
        "modality":"must","action":"retain","target_iri":target,
        "conditions":["The exact approved change is delivered."],"exceptions":[],
        "source":{"title":format!("{title} source"),"uri":format!("{target}:source"),
            "text":source_text},"binding":{"kind":"workspace"},
        "purpose":"execution_constraint","version_resolution":"current_accepted"})
}

async fn preserved_change(pool: &PgPool, change: Uuid) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(\
         'id',id,'unit_id',unit_id,'change_revision',change_revision,'operation',operation,\
         'stage',stage,'expected_generation',expected_generation,\
         'expected_unit_revision',expected_unit_revision,\
         'proposed_unit_revision',proposed_unit_revision,'proposal_digest',proposal_digest,\
         'proposal_fingerprint',proposal_fingerprint,'source_sha256',source_sha256,\
         'semantic_diff',semantic_diff,'baseline',baseline,'proposal',proposal,\
         'binding_provenance',binding_provenance,'preparation_method',preparation_method,\
         'review_method',review_method,'reason',reason,'authority_basis',authority_basis,\
         'review',review,'publication_receipt',publication_receipt) \
         FROM knowledge_changes WHERE id=$1",
    )
    .bind(change)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn preserved_revision(pool: &PgPool, unit: Uuid) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object('tenant_id',tenant_id,'workspace_id',workspace_id,\
         'unit_id',unit_id,'revision',revision,'constraint_payload',constraint_payload,\
         'source_sha256',source_sha256,'rdf_digest',rdf_digest,\
         'rdf_digest_method',rdf_digest_method,'rdf_digest_scope',rdf_digest_scope,\
         'publication_event_id',publication_event_id,'unit_iri',unit_iri,\
         'revision_iri',revision_iri,'source_iri',source_iri,\
         'publication_event_iri',publication_event_iri) \
         FROM knowledge_revisions WHERE unit_id=$1 AND revision=1",
    )
    .bind(unit)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn published_and_approved_dk1_changes_survive_current_upgrade_without_byte_drift() {
    if std::env::var("TECT_TEST_DK1_UPGRADE").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let runtime_role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let legacy_admin = std::env::var("TECT_LEGACY_ADMIN").unwrap();
    let legacy_daemon = std::env::var("TECT_LEGACY_DAEMON").unwrap();
    let legacy_mcp = std::env::var("TECT_LEGACY_MCP").unwrap();
    let base = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(PgConnectOptions::from_str(&admin_url).unwrap())
        .await
        .unwrap();
    let database = format!("dk1_actual_upgrade_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {}", quoted_database(&database)))
        .execute(&base)
        .await
        .unwrap();
    let target_admin = database_url(&admin_url, &database);
    let target_runtime = database_url(&runtime_url, &database);
    run_admin(
        Path::new(&legacy_admin),
        &target_admin,
        &[
            "migrate",
            "--runtime-role",
            &runtime_role,
            "--enable-durable-knowledge",
        ],
    );

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let config = root.join("legacy-host.json");
    run_admin(
        Path::new(&legacy_admin),
        &target_admin,
        &["enroll", "--out", config.to_str().unwrap()],
    );
    let socket = root.join("dk1-upgrade.sock");
    let native = Uuid::new_v4().to_string();
    let workspace = format!("dk1-upgrade-{}", Uuid::new_v4());
    let old_daemon = LegacyDaemon::start(
        Path::new(&legacy_daemon),
        &tagged_url(&target_runtime, "dk1-old-daemon"),
        socket.clone(),
    )
    .await;
    let mut old = LegacyMcp::start(
        Path::new(&legacy_mcp),
        &socket,
        &config,
        &native,
        &workspace,
    )
    .await;
    legacy_route(&mut old, "workspace.open", json!({})).await;
    let published_source = "Exact canonical DK-1 revision remains byte-identical after migration.";
    let published_prepare = legacy_route(
        &mut old,
        "knowledge.change_prepare",
        json!({"request_id":Uuid::new_v4(),"operation":"create","expected_generation":0,
            "draft":legacy_draft("Published DK-1 fixture","urn:tect:target:dk1-published",published_source),
            "reason":"Publish canonical DK-1 state before migration.",
            "authority_basis":"Authenticated legacy workspace owner."}),
    )
    .await;
    let published_change = &published_prepare["prepared"];
    let wrong_review = legacy_route_error(
        &mut old,
        "knowledge.change_review",
        json!({"request_id":Uuid::new_v4(),"change_id":published_change["id"],
            "change_revision":published_change["change_revision"],
            "proposal_digest":published_change["proposal_digest"],"verdict":"approve",
            "review_summary":"Wrong method digest must not authorize publication.",
            "method_read":{"id":published_change["review_method"]["id"],
                "version":published_change["review_method"]["version"],"digest":"wrong-proof"}}),
    )
    .await;
    assert_eq!(wrong_review["error"]["code"], "stale_context");
    let published_review = legacy_route(
        &mut old,
        "knowledge.change_review",
        json!({"request_id":Uuid::new_v4(),"change_id":published_change["id"],
            "change_revision":published_change["change_revision"],
            "proposal_digest":published_change["proposal_digest"],"verdict":"approve",
            "review_summary":"Reviewed exact canonical DK-1 publication.",
            "method_read":{"id":published_change["review_method"]["id"],
                "version":published_change["review_method"]["version"],
                "digest":published_change["review_method"]["digest"]}}),
    )
    .await;
    let published_ready = &published_review["approved"];
    let mut old_peer = LegacyMcp::start(
        Path::new(&legacy_mcp),
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &workspace,
    )
    .await;
    legacy_route(&mut old_peer, "workspace.open", json!({})).await;
    let publish_request = json!({"request_id":Uuid::new_v4(),"change_id":published_ready["id"],
            "change_revision":published_ready["change_revision"],
            "proposal_digest":published_ready["proposal_digest"]});
    let (published_a, published_b) = tokio::join!(
        old.call(
            "command",
            json!({"route":"knowledge.change_publish","params":publish_request.clone()}),
        ),
        old_peer.call(
            "command",
            json!({"route":"knowledge.change_publish","params":publish_request.clone()}),
        )
    );
    assert!(published_a.0 && published_b.0);
    let (old_published, old_replay) = if published_a.1.get("published").is_some() {
        (published_a.1, published_b.1)
    } else {
        (published_b.1, published_a.1)
    };
    assert_eq!(old_replay["replay"], old_published["published"]);
    let old_receipt = old_published["published"].clone();
    assert_eq!(old_receipt["workspace_generation"], 1);
    let old_exact = legacy_query(
        &mut old,
        "knowledge.context",
        json!({"unit_id":old_receipt["unit_id"],"revision":old_receipt["unit_revision"]}),
    )
    .await;
    old_peer.finish().await;
    assert_eq!(
        old_exact["exact_revision"]["constraint"]["source"]["text"],
        published_source
    );
    let source_text = "Exact DK-1 approved proposal survives the DK-2 upgrade byte for byte.";
    let prepare_request = json!({"request_id":Uuid::new_v4(),"operation":"create","expected_generation":1,
            "draft":legacy_draft("Pending DK-1 fixture","urn:tect:target:dk1-pending",source_text),
            "reason":"Prove an actual in-flight DK-1 change.",
            "authority_basis":"Authenticated legacy workspace owner."});
    let prepared = legacy_route(
        &mut old,
        "knowledge.change_prepare",
        prepare_request.clone(),
    )
    .await;
    let change = &prepared["prepared"];
    let approved = legacy_route(
        &mut old,
        "knowledge.change_review",
        json!({"request_id":Uuid::new_v4(),"change_id":change["id"],
            "change_revision":change["change_revision"],"proposal_digest":change["proposal_digest"],
            "verdict":"approve","review_summary":"Reviewed the exact DK-1 proposal before upgrade.",
            "method_read":{"id":change["review_method"]["id"],
                "version":change["review_method"]["version"],"digest":change["review_method"]["digest"]}}),
    )
    .await;
    let ready = approved["approved"].clone();
    old.finish().await;
    old_daemon.stop().await;

    let pool = connect_database(&admin_url, &database).await;
    let change_id = Uuid::parse_str(ready["id"].as_str().unwrap()).unwrap();
    let before = preserved_change(&pool, change_id).await;
    let published_unit = Uuid::parse_str(old_receipt["unit_id"].as_str().unwrap()).unwrap();
    let revision_before = preserved_revision(&pool, published_unit).await;
    let (tenant, workspace_id): (Uuid, Uuid) =
        sqlx::query_as("SELECT tenant_id,id FROM workspaces WHERE key=$1")
            .bind(&workspace)
            .fetch_one(&pool)
            .await
            .unwrap();
    let graph = format!("urn:tect:dk:workspace:{tenant}:{workspace_id}");
    let graph_before: String = sqlx::query_scalar("SELECT pgrdf.graph_digest(pgrdf.graph_id($1))")
        .bind(&graph)
        .fetch_one(&pool)
        .await
        .unwrap();
    admin::migrate(&pool, &runtime_role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &runtime_role)
        .await
        .unwrap();
    assert_eq!(preserved_change(&pool, change_id).await, before);
    let revision_after = preserved_revision(&pool, published_unit).await;
    assert_eq!(revision_after, revision_before);
    let graph_after: String = sqlx::query_scalar("SELECT pgrdf.graph_digest(pgrdf.graph_id($1))")
        .bind(&graph)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(graph_after, graph_before);

    let daemon = Daemon::start(
        &tagged_url(&target_runtime, "dk1-current-daemon"),
        socket.clone(),
    )
    .await;
    let mut current = Mcp::start(&socket, &config, &native, &workspace).await;
    current.call("open_workspace", json!({})).await;
    let current_old_exact = route(
        &mut current,
        "query",
        "knowledge.context",
        json!({"unit_id":old_receipt["unit_id"],"revision":old_receipt["unit_revision"]}),
    )
    .await;
    assert_eq!(
        current_old_exact["exact_revision"],
        old_exact["exact_revision"]
    );
    assert_eq!(
        route(
            &mut current,
            "command",
            "knowledge.change_prepare",
            prepare_request.clone()
        )
        .await["replay"],
        prepared["prepared"]
    );
    let mut conflicting_prepare = prepare_request.clone();
    conflicting_prepare["reason"] = json!("Changed payload under the saved DK-1 request ID.");
    assert_eq!(
        route_error(
            &mut current,
            "command",
            "knowledge.change_prepare",
            conflicting_prepare
        )
        .await["error"]["code"],
        "input_conflict"
    );
    let published = route(
        &mut current,
        "command",
        "knowledge.change_publish",
        json!({"request_id":Uuid::new_v4(),"change_id":ready["id"],
            "change_revision":ready["change_revision"],"proposal_digest":ready["proposal_digest"]}),
    )
    .await;
    let receipt = &published["published"];
    assert_eq!(receipt["workspace_generation"], 2);
    let exact = route(
        &mut current,
        "query",
        "knowledge.context",
        json!({"unit_id":receipt["unit_id"],"revision":receipt["unit_revision"]}),
    )
    .await;
    assert_eq!(exact["generation"], 2);
    assert_eq!(
        exact["exact_revision"]["constraint"]["source"]["text"],
        source_text
    );
    let mut fresh_prepare = prepare_request;
    fresh_prepare["request_id"] = json!(Uuid::new_v4());
    fresh_prepare["expected_generation"] = json!(2);
    assert_eq!(
        route_error(
            &mut current,
            "command",
            "knowledge.change_prepare",
            fresh_prepare
        )
        .await["error"]["code"],
        "knowledge_lifecycle_required"
    );
    current.finish().await;
    drop(daemon);
    pool.close().await;
    sqlx::query(&format!(
        "DROP DATABASE {} WITH (FORCE)",
        quoted_database(&database)
    ))
    .execute(&base)
    .await
    .unwrap();
}
