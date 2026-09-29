//! Production command round trip against a dedicated disposable PostgreSQL 18 + pgRDF database.
use serde_json::Value;
use sqlx::PgPool;
use std::{fs, os::unix::fs::PermissionsExt};
use tokio::process::Command;
use uuid::Uuid;

fn database_url(url: &str, name: &str) -> String {
    let mut parsed = url::Url::parse(url).unwrap();
    parsed.set_path(&format!("/{name}"));
    parsed.into()
}

async fn command(url: &str, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tect-admin"))
        .env("TECT_ADMIN_DATABASE_URL", url)
        .args(args)
        .output()
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn production_restore_stages_copied_oid_and_exact_graphs_without_runtime_connect() {
    if std::env::var("TECT_TEST_DK_STAGED").as_deref() != Ok("1") {
        return;
    }
    let source_url = std::env::var("TECT_TEST_ADMIN_URL").expect("dedicated PG18 admin URL");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("dedicated runtime role");
    let source = PgPool::connect(&source_url).await.unwrap();
    tect_postgres::admin::migrate(&source, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&source, &role)
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let bundle = temp.path().canonicalize().unwrap().join("bundle");
    let backup = command(
        &source_url,
        &[
            "backup",
            "--out",
            bundle.to_str().unwrap(),
            "--runtime-role",
            &role,
        ],
    )
    .await;
    assert!(
        backup.status.success(),
        "{}",
        String::from_utf8_lossy(&backup.stderr)
    );

    let database = format!("tect_staged_{}", Uuid::new_v4().simple());
    let bare = command(
        &source_url,
        &[
            "restore",
            "--from",
            bundle.to_str().unwrap(),
            "--database",
            &database,
            "--runtime-role",
            &role,
        ],
    )
    .await;
    assert!(!bare.status.success());
    let maintenance = PgPool::connect(&database_url(&source_url, "postgres"))
        .await
        .unwrap();
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname=$1)")
            .bind(&database)
            .fetch_one(&maintenance)
            .await
            .unwrap();
    assert!(!exists, "bare restore created a database");

    let restored = command(
        &source_url,
        &[
            "restore",
            "--staged",
            "--from",
            bundle.to_str().unwrap(),
            "--database",
            &database,
            "--runtime-role",
            &role,
        ],
    )
    .await;
    assert!(
        restored.status.success(),
        "{}",
        String::from_utf8_lossy(&restored.stderr)
    );
    assert!(String::from_utf8_lossy(&restored.stdout).contains("staged and sealed"));
    let target = PgPool::connect(&database_url(&source_url, &database))
        .await
        .unwrap();
    tect_postgres::admin::validate_staged_restore(&target, &role)
        .await
        .unwrap();
    let mut runtime_url = url::Url::parse(&database_url(&source_url, &database)).unwrap();
    runtime_url.set_username(&role).unwrap();
    runtime_url.set_password(None).unwrap();
    assert!(
        PgPool::connect(runtime_url.as_str()).await.is_err(),
        "runtime role connected to staged database"
    );
    let (source_oid, target_oid, ready): (i64, i64, bool) = sqlx::query_as(
        "SELECT c.qualified_database_oid::bigint,d.oid::bigint,
                public.tect_dk_database_identity_ready()
         FROM durable_knowledge_capability c
         JOIN pg_database d ON d.datname=current_database() WHERE c.singleton",
    )
    .fetch_one(&target)
    .await
    .unwrap();
    assert_ne!(source_oid, target_oid);
    assert!(!ready);
    assert!(
        tect_postgres::enable_durable_knowledge(&target, &role)
            .await
            .is_err(),
        "ordinary enable bypassed copied identity guard"
    );

    let manifest: Value =
        serde_json::from_slice(&fs::read(bundle.join("manifest.json")).unwrap()).unwrap();
    let graphs = manifest["graphs"].as_array().unwrap();
    assert!(!graphs.is_empty());
    let inventory: Vec<String> =
        sqlx::query_scalar("SELECT iri FROM pgrdf.graph_inventory() ORDER BY iri")
            .fetch_all(&target)
            .await
            .unwrap();
    let mut expected: Vec<String> = graphs
        .iter()
        .map(|graph| graph["iri"].as_str().unwrap().into())
        .collect();
    expected.sort();
    assert_eq!(inventory, expected);
    for graph in graphs {
        let digest: String = sqlx::query_scalar("SELECT pgrdf.graph_digest(pgrdf.graph_id($1))")
            .bind(graph["iri"].as_str().unwrap())
            .fetch_one(&target)
            .await
            .unwrap();
        assert_eq!(digest, graph["native_digest"].as_str().unwrap());
    }
    target.close().await;
    sqlx::query(&format!("DROP DATABASE \"{database}\" WITH (FORCE)"))
        .execute(&maintenance)
        .await
        .unwrap();
    maintenance.close().await;
    source.close().await;
}
