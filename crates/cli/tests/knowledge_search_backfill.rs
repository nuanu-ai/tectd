#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::commit_create;
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{path::Path, process::Stdio};
use tect_postgres::admin;
use tokio::process::Command;
use uuid::Uuid;

fn document() -> Value {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/runbook.json"
    ))
    .unwrap();
    value["document"]["title"] = json!("Pre-DK3 canonical searchable title");
    value["document"]["canonical_text"] =
        json!("This canonical record predates the search projection migration.");
    value["document"]["sources"][0]["snapshot"]["uri"] = json!("urn:dk3-backfill:pre-search");
    value["document"]["sources"][0]["snapshot"]["text"] = json!("Pinned pre-search source text.");
    value["document"].clone()
}

async fn sha256(path: &Path) -> String {
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .into()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn pre_dk3_verified_unit_is_backfilled_for_lexical_search() {
    if std::env::var("TECT_TEST_DK3_BACKFILL").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let pool = PgPool::connect(&admin_url).await.unwrap();
    let dk2 = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tectd-dk2/target/debug")
        .canonicalize()
        .unwrap();
    let old_admin = dk2.join("tect-admin");
    let old_daemon = dk2.join("tectd");
    let old_mcp = dk2.join("tectd-mcp");
    assert_eq!(
        sha256(&old_admin).await,
        "9fef5216410835bf4c1752906e1eb0ea27191448692c01451becd3913eccd7d0"
    );
    assert_eq!(
        sha256(&old_daemon).await,
        "0c0156dda453c9c32b1a39d89b654331dc5b1b90c5c10bbfc56fa6e175d13718"
    );
    assert_eq!(
        sha256(&old_mcp).await,
        "f8cdfe9ce5f4142016d9b54e29e33a9246f251c08013ab6304652327d145075c"
    );
    let status = Command::new(&old_admin)
        .env("TECT_ADMIN_DATABASE_URL", &admin_url)
        .args([
            "migrate",
            "--runtime-role",
            &role,
            "--enable-durable-knowledge",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .unwrap();
    assert!(status.success());

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    support::repository(&repo);
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let socket = root.join("dk3-backfill.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("dk3-backfill-old-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start_with(&old_daemon, &runtime, socket.clone()).await;
    let workspace_key = format!("dk3-backfill-{}", Uuid::new_v4());
    let native = Uuid::new_v4().to_string();
    let mut client = Mcp::start_with(&old_mcp, &socket, &config, &native, &workspace_key).await;
    support::route(&mut client, "command", "workspace.open", json!({})).await;
    let committed = commit_create(&mut client, document()).await;
    let unit = committed.receipt["applied_operations"][0]["unit_id"].clone();
    let absent: Option<String> = sqlx::query_scalar(
        "SELECT pg_catalog.to_regclass('public.knowledge_search_resources')::text",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(absent, None);
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();

    admin::migrate(&pool, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();
    let projection: (i64, String) =
        sqlx::query_as("SELECT revision,title FROM knowledge_search_resources WHERE unit_id=$1")
            .bind(Uuid::parse_str(unit.as_str().unwrap()).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(projection, (1, "Pre-DK3 canonical searchable title".into()));

    let socket = root.join("dk3-backfill-current.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("dk3-backfill-new-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let mut client = Mcp::start(&socket, &config, &native, &workspace_key).await;
    let response = support::route(
        &mut client,
        "query",
        "knowledge.search",
        json!({"mode":"lexical","query":"canonical searchable title","limit":5,
            "corpus_limit":32,"purpose":"Prove verified pre-DK3 lexical backfill."}),
    )
    .await;
    assert_eq!(response["vector_status"], "not_requested");
    assert_eq!(response["results"][0]["unit_id"], unit);
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    pool.close().await;
}
