//! Production command round trip against a dedicated disposable PostgreSQL 18 + pgRDF database.
use serde_json::Value;
use sqlx::PgPool;
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
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

fn copy_bundle(source: &Path, destination: &Path) {
    fs::create_dir(destination).unwrap();
    fs::set_permissions(destination, fs::Permissions::from_mode(0o700)).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_bundle(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
            fs::set_permissions(target, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
}

async fn assert_runtime_connect_denied(runtime_url: &str, database: &str) {
    let error = PgPool::connect(&database_url(runtime_url, database))
        .await
        .expect_err("runtime role connected to sealed database");
    let database_error = error
        .as_database_error()
        .expect("expected PostgreSQL CONNECT denial, not an authentication or transport error");
    assert_eq!(database_error.code().as_deref(), Some("42501"));
    assert!(
        database_error
            .message()
            .contains("permission denied for database")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn production_restore_stages_copied_oid_and_exact_graphs_without_runtime_connect() {
    if std::env::var("TECT_TEST_DK_STAGED").as_deref() != Ok("1") {
        return;
    }
    let source_url = std::env::var("TECT_TEST_ADMIN_URL").expect("dedicated PG18 admin URL");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("dedicated runtime role");
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").expect("valid runtime credentials");
    let source = PgPool::connect(&source_url).await.unwrap();
    let runtime = PgPool::connect(&runtime_url)
        .await
        .expect("runtime credentials must connect to the source database");
    let authenticated_role: String = sqlx::query_scalar("SELECT CURRENT_USER")
        .fetch_one(&runtime)
        .await
        .unwrap();
    assert_eq!(authenticated_role, role);
    runtime.close().await;
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
    assert_runtime_connect_denied(&runtime_url, &database).await;
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

    // Keep the bundle internally consistent, but make its native digest wrong.
    // The failure must happen after target creation, and the target must stay sealed.
    let bad_bundle = temp.path().canonicalize().unwrap().join("bad-bundle");
    copy_bundle(&bundle, &bad_bundle);
    let manifest_path = bad_bundle.join("manifest.json");
    let mut bad_manifest: Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let original_digest = bad_manifest["graphs"][0]["native_digest"].as_str().unwrap();
    let wrong_digest = if original_digest == "0".repeat(64) {
        "1".repeat(64)
    } else {
        "0".repeat(64)
    };
    bad_manifest["graphs"][0]["native_digest"] = Value::from(wrong_digest);
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&bad_manifest).unwrap(),
    )
    .unwrap();
    let failed_database = format!("tect_staged_fail_{}", Uuid::new_v4().simple());
    let failed = command(
        &source_url,
        &[
            "restore",
            "--staged",
            "--from",
            bad_bundle.to_str().unwrap(),
            "--database",
            &failed_database,
            "--runtime-role",
            &role,
        ],
    )
    .await;
    assert!(
        !failed.status.success(),
        "changed native digest was accepted"
    );
    let failed_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname=$1)")
            .bind(&failed_database)
            .fetch_one(&maintenance)
            .await
            .unwrap();
    assert!(failed_exists, "failure happened before target creation");
    let failed_target = PgPool::connect(&database_url(&source_url, &failed_database))
        .await
        .unwrap();
    tect_postgres::admin::validate_staged_restore(&failed_target, &role)
        .await
        .unwrap();
    assert_runtime_connect_denied(&runtime_url, &failed_database).await;
    failed_target.close().await;
    target.close().await;
    sqlx::query(&format!("DROP DATABASE \"{failed_database}\" WITH (FORCE)"))
        .execute(&maintenance)
        .await
        .unwrap();
    sqlx::query(&format!("DROP DATABASE \"{database}\" WITH (FORCE)"))
        .execute(&maintenance)
        .await
        .unwrap();
    maintenance.close().await;
    source.close().await;
}
