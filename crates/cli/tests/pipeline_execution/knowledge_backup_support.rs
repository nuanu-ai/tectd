use super::recovery_support::{Daemon, Mcp};
use super::support::{route, route_error};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{fs, path::Path};
use tokio::process::Command;
use uuid::Uuid;

#[derive(serde::Serialize)]
struct GraphBackup {
    iri: String,
    digest: String,
    file: String,
}

fn database_url(url: &str, database: &str) -> String {
    let (base, query) = url
        .split_once('?')
        .map_or((url, None), |(a, b)| (a, Some(b)));
    let slash = base.rfind('/').expect("PostgreSQL URL has a database path");
    let mut value = format!("{}/{database}", &base[..slash]);
    if let Some(query) = query {
        value.push('?');
        value.push_str(query);
    }
    value
}

fn quoted_database(name: &str) -> String {
    assert!(
        name.bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    format!("\"{name}\"")
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn assert_application_roundtrip(
    source_pool: &PgPool,
    admin_url: &str,
    runtime_url: &str,
    socket_dir: &Path,
    config: &Path,
    native: &str,
    workspace_key: &str,
    runtime_role: &str,
    unit_id: &Value,
    expected: &Value,
) {
    let suppression = tect_postgres::prepare_knowledge_suppression_manifest(source_pool)
        .await
        .unwrap();
    assert_eq!(suppression.high_water_erasure_sequence, 0);
    let checkpoint = tect_postgres::record_knowledge_suppression_export(source_pool, &suppression)
        .await
        .unwrap();
    let source_database: String = sqlx::query_scalar("SELECT pg_catalog.current_database()")
        .fetch_one(source_pool)
        .await
        .unwrap();
    let database = format!("tect_dk_restore_{}", Uuid::new_v4().simple());
    let maintenance_url = database_url(admin_url, "postgres");
    let maintenance = PgPool::connect(&maintenance_url).await.unwrap();
    sqlx::query(&format!("CREATE DATABASE {}", quoted_database(&database)))
        .execute(&maintenance)
        .await
        .unwrap();
    let dump = socket_dir.join("application-dk.dump");
    let graph_dir = socket_dir.join("application-dk-graphs");
    fs::create_dir(&graph_dir).unwrap();
    let mut snapshot = source_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *snapshot)
        .await
        .unwrap();
    let snapshot_id: String = sqlx::query_scalar("SELECT pg_catalog.pg_export_snapshot()")
        .fetch_one(&mut *snapshot)
        .await
        .unwrap();
    let inventory: Vec<(i64, String)> =
        sqlx::query_as("SELECT graph_id,iri FROM pgrdf.graph_inventory() ORDER BY iri")
            .fetch_all(&mut *snapshot)
            .await
            .unwrap();
    assert!(
        inventory
            .iter()
            .all(|(_, iri)| !iri.starts_with("urn:tect:dk:scratch:")),
        "publisher scratch graphs must not become durable backup inputs"
    );
    let mut graphs = Vec::new();
    for (index, (graph_id, iri)) in inventory.iter().enumerate() {
        let lines: Vec<String> = sqlx::query_scalar("SELECT * FROM pgrdf.export_graph($1)")
            .bind(graph_id)
            .fetch_all(&mut *snapshot)
            .await
            .unwrap();
        let payload = if lines.is_empty() {
            String::new()
        } else {
            lines.join("\n") + "\n"
        };
        let digest: String = sqlx::query_scalar("SELECT pgrdf.graph_digest($1)")
            .bind(graph_id)
            .fetch_one(&mut *snapshot)
            .await
            .unwrap();
        let file = format!("graph-{index}.nt");
        fs::write(graph_dir.join(&file), payload).unwrap();
        graphs.push(GraphBackup {
            iri: iri.clone(),
            digest,
            file,
        });
    }
    fs::write(
        graph_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&graphs).unwrap(),
    )
    .unwrap();
    let dump_status = Command::new("pg_dump")
        .args([
            "--no-password",
            "--format=custom",
            "--exclude-schema=pgrdf",
            "--exclude-extension=pgrdf",
            "--snapshot",
        ])
        .arg(&snapshot_id)
        .arg("--file")
        .arg(&dump)
        .arg("--dbname")
        .arg(database_url(admin_url, &source_database))
        .status()
        .await
        .expect("pg_dump is required for the opt-in DK test");
    assert!(dump_status.success(), "application pg_dump failed");
    snapshot.commit().await.unwrap();
    let toc = Command::new("pg_restore")
        .args(["--list"])
        .arg(&dump)
        .output()
        .await
        .unwrap();
    assert!(
        toc.status.success(),
        "application dump TOC inspection failed"
    );
    let toc = String::from_utf8(toc.stdout).unwrap();
    assert!(!toc.contains("_pgrdf_"));
    assert!(!toc.contains("TABLE DATA pgrdf "));
    assert!(!toc.contains("TABLE pgrdf "));
    let restored_admin_url = database_url(admin_url, &database);
    let restore_status = Command::new("pg_restore")
        .args(["--no-password", "--exit-on-error", "--no-owner", "--dbname"])
        .arg(&restored_admin_url)
        .arg(&dump)
        .status()
        .await
        .expect("pg_restore is required for the opt-in DK test");
    assert!(restore_status.success(), "application pg_restore failed");
    let restored_pool = PgPool::connect(&restored_admin_url).await.unwrap();
    assert!(
        tect_postgres::enable_durable_knowledge(&restored_pool, runtime_role)
            .await
            .is_err(),
        "ordinary enable must refuse a restored qualified database identity"
    );
    let socket = socket_dir.join("restored-knowledge-blocked.sock");
    let restored_runtime_url = database_url(runtime_url, &database);
    let mut blocked_daemon = Daemon::start(&restored_runtime_url, socket.clone()).await;
    let mut blocked_client = Mcp::start(&socket, config, native, workspace_key).await;
    blocked_client.call("open_workspace", json!({})).await;
    let blocked = route_error(
        &mut blocked_client,
        "query",
        "knowledge.context",
        json!({"unit_id":unit_id,"revision":1}),
    )
    .await;
    assert_eq!(blocked["error"]["code"], "knowledge_unavailable");
    blocked_client.finish().await;
    blocked_daemon.crash().await;
    blocked_daemon.remove_owned_stale_socket();

    sqlx::query("CREATE EXTENSION pgrdf VERSION '0.6.34'")
        .execute(&restored_pool)
        .await
        .unwrap();
    for graph in &graphs {
        let existing: Option<i64> = sqlx::query_scalar("SELECT pgrdf.graph_id($1)")
            .bind(&graph.iri)
            .fetch_one(&restored_pool)
            .await
            .unwrap();
        let graph_id = match existing {
            Some(graph_id) => graph_id,
            None => sqlx::query_scalar("SELECT pgrdf.add_graph($1)")
                .bind(&graph.iri)
                .fetch_one(&restored_pool)
                .await
                .unwrap(),
        };
        let current: String = sqlx::query_scalar("SELECT pgrdf.graph_digest($1)")
            .bind(graph_id)
            .fetch_one(&restored_pool)
            .await
            .unwrap();
        if current != graph.digest {
            let current_lines: Vec<String> =
                sqlx::query_scalar("SELECT * FROM pgrdf.export_graph($1)")
                    .bind(graph_id)
                    .fetch_all(&restored_pool)
                    .await
                    .unwrap();
            assert!(
                current_lines.is_empty(),
                "activation produced a conflicting graph for {}",
                graph.iri
            );
            let payload = fs::read_to_string(graph_dir.join(&graph.file)).unwrap();
            if !payload.is_empty() {
                sqlx::query("SELECT pgrdf.parse_turtle($1,$2)")
                    .bind(payload)
                    .bind(graph_id)
                    .execute(&restored_pool)
                    .await
                    .unwrap();
            }
        }
        let restored_digest: String = sqlx::query_scalar("SELECT pgrdf.graph_digest($1)")
            .bind(graph_id)
            .fetch_one(&restored_pool)
            .await
            .unwrap();
        assert_eq!(
            restored_digest, graph.digest,
            "graph digest changed for {}",
            graph.iri
        );
    }
    let recovered = tect_postgres::apply_knowledge_suppression_manifest(
        &restored_pool,
        &suppression,
        &checkpoint,
    )
    .await
    .unwrap();
    assert_eq!(recovered.entries_applied, 0);
    assert_eq!(recovered.units_suppressed, 0);
    assert_eq!(recovered.remaining, 0);

    let socket = socket_dir.join("restored-knowledge.sock");
    let mut daemon = Daemon::start(&restored_runtime_url, socket.clone()).await;
    let mut client = Mcp::start(&socket, config, native, workspace_key).await;
    let restored = route(
        &mut client,
        "query",
        "knowledge.context",
        json!({"unit_id":unit_id,"revision":1}),
    )
    .await;
    let actual = restored["exact_revision"].clone();
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    restored_pool.close().await;
    sqlx::query(&format!(
        "DROP DATABASE {} WITH (FORCE)",
        quoted_database(&database)
    ))
    .execute(&maintenance)
    .await
    .unwrap();
    maintenance.close().await;

    for field in [
        "unit_id",
        "revision",
        "constraint",
        "source_sha256",
        "rdf_digest",
        "unit_iri",
        "revision_iri",
        "source_iri",
        "publication_event_iri",
        "binding_provenance",
    ] {
        assert_eq!(actual[field], expected[field], "restore changed {field}");
    }
}
