use super::recovery_support::{Daemon, Mcp};
use super::support::{route, route_error};
#[path = "knowledge_search_restore_support.rs"]
mod knowledge_search_restore_support;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{
    env, fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tect_domain::KnowledgeEmbeddingJobCompletion;
use tect_postgres::{KnowledgeSuppressionCheckpoint, KnowledgeSuppressionManifest};
use tokio::process::Command;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
struct GraphBackup {
    iri: String,
    digest: String,
    file: String,
}

pub struct ApplicationBackup {
    dump: PathBuf,
    graph_dir: PathBuf,
}

fn database_url(url: &str, database: &str) -> String {
    let (base, query) = url
        .split_once('?')
        .map_or((url, None), |(a, b)| (a, Some(b)));
    let slash = base.rfind('/').unwrap();
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
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    );
    format!("\"{name}\"")
}

fn restore_pgpass(directory: &Path, database: &str) -> PathBuf {
    let source = env::var("PGPASSFILE").expect("PGPASSFILE is required for managed restore");
    let source = fs::read_to_string(source).expect("read source pgpass");
    let mut restored = String::new();
    for line in source
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let fields: Vec<&str> = line.splitn(5, ':').collect();
        assert_eq!(fields.len(), 5, "invalid pgpass entry");
        restored.push_str(&format!(
            "{}:{}:{}:{}:{}\n",
            fields[0], fields[1], database, fields[3], fields[4]
        ));
    }
    assert!(!restored.is_empty(), "source pgpass has no usable entry");
    let path = directory.join(format!("restore-{}.pgpass", Uuid::new_v4().simple()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    file.write_all(restored.as_bytes()).unwrap();
    path
}

fn pgpass_password(path: &Path, user: &str) -> String {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .find_map(|line| {
            let fields: Vec<&str> = line.splitn(5, ':').collect();
            (fields.len() == 5 && fields[3] == user).then(|| fields[4].to_owned())
        })
        .expect("restore pgpass entry for user")
}

fn authenticated_url(url: &str, password: &str) -> String {
    let password = password.bytes().fold(String::new(), |mut value, byte| {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            value.push(char::from(byte));
        } else {
            value.push_str(&format!("%{byte:02X}"));
        }
        value
    });
    let user_end = url.find('@').expect("database URL user separator");
    format!("{}:{password}{}", &url[..user_end], &url[user_end..])
}

pub async fn capture(pool: &PgPool, admin_url: &str, directory: &Path) -> ApplicationBackup {
    let source_database: String = sqlx::query_scalar("SELECT pg_catalog.current_database()")
        .fetch_one(pool)
        .await
        .unwrap();
    let dump = directory.join("suppression-application.dump");
    let graph_dir = directory.join("suppression-graphs");
    fs::create_dir(&graph_dir).unwrap();
    let mut snapshot = pool.begin().await.unwrap();
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
            .all(|(_, iri)| !iri.starts_with("urn:tect:dk:scratch:"))
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
    let status = Command::new("pg_dump")
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
        .unwrap();
    assert!(status.success());
    snapshot.commit().await.unwrap();
    ApplicationBackup { dump, graph_dir }
}

#[allow(clippy::too_many_arguments)]
pub async fn restore_apply_and_verify(
    backup: ApplicationBackup,
    admin_url: &str,
    runtime_url: &str,
    directory: &Path,
    config: &Path,
    native: &str,
    workspace_key: &str,
    runtime_role: &str,
    manifest: &KnowledgeSuppressionManifest,
    checkpoint: &KnowledgeSuppressionCheckpoint,
    older_manifest: &KnowledgeSuppressionManifest,
    older_checkpoint: &KnowledgeSuppressionCheckpoint,
    erased_unit: &Value,
    erased_begin_request: &Value,
    cached_run: &Value,
    erased_marker: &str,
    survivor_unit: &Value,
    survivor_expected: &Value,
    restored_completion: &KnowledgeEmbeddingJobCompletion,
) {
    let database = format!("tect_dk_suppression_restore_{}", Uuid::new_v4().simple());
    let maintenance = PgPool::connect(&database_url(admin_url, "postgres"))
        .await
        .unwrap();
    sqlx::query(&format!("CREATE DATABASE {}", quoted_database(&database)))
        .execute(&maintenance)
        .await
        .unwrap();
    let restored_admin_url = database_url(admin_url, &database);
    let restore_pgpass = restore_pgpass(directory, &database);
    let status = Command::new("pg_restore")
        .args(["--no-password", "--exit-on-error", "--no-owner", "--dbname"])
        .arg(&restored_admin_url)
        .arg(&backup.dump)
        .env("PGPASSFILE", &restore_pgpass)
        .status()
        .await
        .unwrap();
    if !status.success() {
        fs::remove_file(&restore_pgpass).unwrap();
        sqlx::query(&format!(
            "DROP DATABASE {} WITH (FORCE)",
            quoted_database(&database)
        ))
        .execute(&maintenance)
        .await
        .unwrap();
    }
    assert!(status.success());
    let admin_password = pgpass_password(&restore_pgpass, env::var("PGUSER").unwrap().as_str());
    let admin_options = restored_admin_url
        .parse::<PgConnectOptions>()
        .unwrap()
        .password(&admin_password);
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(admin_options)
        .await
        .unwrap();
    assert!(
        tect_postgres::enable_durable_knowledge(&pool, runtime_role)
            .await
            .is_err()
    );
    sqlx::query("CREATE EXTENSION pgrdf VERSION '0.6.34'")
        .execute(&pool)
        .await
        .unwrap();
    let graphs: Vec<GraphBackup> =
        serde_json::from_slice(&fs::read(backup.graph_dir.join("manifest.json")).unwrap()).unwrap();
    for graph in &graphs {
        let graph_id: i64 = sqlx::query_scalar("SELECT pgrdf.add_graph($1)")
            .bind(&graph.iri)
            .fetch_one(&pool)
            .await
            .unwrap();
        let payload = fs::read_to_string(backup.graph_dir.join(&graph.file)).unwrap();
        if !payload.is_empty() {
            sqlx::query("SELECT pgrdf.parse_turtle($1,$2)")
                .bind(payload)
                .bind(graph_id)
                .execute(&pool)
                .await
                .unwrap();
        }
        let digest: String = sqlx::query_scalar("SELECT pgrdf.graph_digest($1)")
            .bind(graph_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(digest, graph.digest);
    }
    assert!(
        tect_postgres::enable_durable_knowledge(&pool, runtime_role)
            .await
            .is_err()
    );
    let runtime = database_url(runtime_url, &database);
    let runtime_password = pgpass_password(&restore_pgpass, runtime_role);
    let runtime = authenticated_url(&runtime, &runtime_password);
    let (tenant, workspace, event): (Uuid, Uuid, Uuid) = sqlx::query_as(
        "SELECT tenant_id,workspace_id,id FROM knowledge_publication_events \
         WHERE unit_id=$1 AND unit_revision=1 AND NOT payload_erased ORDER BY created_at DESC LIMIT 1",
    )
    .bind(Uuid::parse_str(survivor_unit.as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    let runtime_pool = PgPool::connect(&runtime).await.unwrap();
    let blocked_native =
        sqlx::query("SELECT * FROM public.tect_dk2_native_read($1,$2,$3,1,$4,true)")
            .bind(tenant)
            .bind(workspace)
            .bind(Uuid::parse_str(survivor_unit.as_str().unwrap()).unwrap())
            .bind(event)
            .execute(&runtime_pool)
            .await;
    assert!(blocked_native.is_err());
    runtime_pool.close().await;
    knowledge_search_restore_support::verify_blocked(&pool, &runtime, restored_completion).await;
    let blocked_socket = directory.join("suppression-blocked.sock");
    let mut blocked_daemon = Command::new(env!("CARGO_BIN_EXE_tectd"))
        .env("TECT_DATABASE_URL", &runtime)
        .env("TECT_SOCKET", &blocked_socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let status = tokio::time::timeout(Duration::from_secs(5), blocked_daemon.wait())
        .await
        .expect("restored runtime must fail closed without waiting")
        .unwrap();
    assert!(!status.success());
    assert!(older_manifest.high_water_erasure_sequence < manifest.high_water_erasure_sequence);
    assert_eq!(
        older_checkpoint.erasure_sequence,
        older_manifest.high_water_erasure_sequence
    );
    assert!(
        tect_postgres::apply_knowledge_suppression_manifest(&pool, older_manifest, checkpoint)
            .await
            .is_err()
    );
    let mut bytes = tect_postgres::knowledge_suppression_manifest_bytes(manifest).unwrap();
    bytes.truncate(bytes.len() / 2);
    assert!(tect_postgres::parse_knowledge_suppression_manifest(&bytes).is_err());
    let report = tect_postgres::apply_knowledge_suppression_manifest(&pool, manifest, checkpoint)
        .await
        .unwrap();
    assert_eq!(report.remaining, 0);
    assert_eq!(report.units_suppressed, manifest.entries.len() as i64);
    knowledge_search_restore_support::verify_requalified(
        &pool,
        &runtime,
        config,
        workspace,
        Uuid::parse_str(survivor_unit.as_str().unwrap()).unwrap(),
        restored_completion,
        runtime_role,
    )
    .await;
    let entry = manifest
        .entries
        .iter()
        .find(|entry| entry.unit_id.to_string() == erased_unit.as_str().unwrap())
        .unwrap();
    let erased_search: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM knowledge_search_resources WHERE unit_id=$1),\
         (SELECT count(*) FROM knowledge_search_embedding_jobs WHERE unit_id=$1),\
         (SELECT count(*) FROM knowledge_search_vectors WHERE unit_id=$1)",
    )
    .bind(entry.unit_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(erased_search, (0, 0, 0));
    let result_copies: (i64, i64) = sqlx::query_as(
        "SELECT count(*),count(*) FILTER (WHERE r.payload_erased AND r.summary IS NULL \
         AND r.evidence IS NULL AND r.knowledge_publisher_receipt_digest IS NULL) \
         FROM knowledge_owned_copies c JOIN slice_results r ON r.tenant_id=c.tenant_id \
         AND r.workspace_id=c.workspace_id AND r.id=c.row_id WHERE c.tenant_id=$1 \
         AND c.workspace_id=$2 AND c.unit_id=$3 AND c.relation_name='slice_results'",
    )
    .bind(entry.tenant_id)
    .bind(entry.workspace_id)
    .bind(entry.unit_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let planning_copies: (i64, i64) = sqlx::query_as(
        "SELECT count(*),count(*) FILTER (WHERE i.payload_erased AND i.input IS NULL) \
         FROM knowledge_owned_copies c JOIN slice_planning_inputs i ON i.tenant_id=c.tenant_id \
         AND i.workspace_id=c.workspace_id AND i.id=c.row_id WHERE c.tenant_id=$1 \
         AND c.workspace_id=$2 AND c.unit_id=$3 AND c.relation_name='slice_planning_inputs'",
    )
    .bind(entry.tenant_id)
    .bind(entry.workspace_id)
    .bind(entry.unit_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let run_copies: (i64, i64) = sqlx::query_as(
        "SELECT count(*),count(*) FILTER (WHERE r.payload_erased AND r.origin_payload IS NULL \
         AND r.origin_result IS NULL AND r.qualification_reason IS NULL) \
         FROM knowledge_owned_copies c JOIN slice_pipeline_runs r ON r.tenant_id=c.tenant_id \
         AND r.workspace_id=c.workspace_id AND r.id=c.row_id WHERE c.tenant_id=$1 \
         AND c.workspace_id=$2 AND c.unit_id=$3 AND c.relation_name='slice_pipeline_runs'",
    )
    .bind(entry.tenant_id)
    .bind(entry.workspace_id)
    .bind(entry.unit_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    for (total, clean) in [result_copies, planning_copies, run_copies] {
        assert!(total > 0);
        assert_eq!(clean, total);
    }
    let generation: i64 = sqlx::query_scalar(
        "SELECT generation FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(entry.tenant_id)
    .bind(entry.workspace_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let marker_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pgrdf._pgrdf_dictionary WHERE lexical_value=$1")
            .bind(erased_marker)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(marker_count, 0);

    let socket = directory.join("suppression-recovered.sock");
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let mut client = Mcp::start(&socket, config, native, workspace_key).await;
    let erased = route(
        &mut client,
        "query",
        "knowledge.unit",
        json!({"unit_id":erased_unit,"revision":1}),
    )
    .await;
    assert_eq!(erased["payload_erased"]["unit_id"], *erased_unit);
    assert!(!erased.to_string().contains(erased_marker));
    assert_eq!(
        route_error(
            &mut client,
            "query",
            "slice.pipeline.context",
            json!({"run_id":cached_run})
        )
        .await["error"]["code"],
        "knowledge_payload_erased"
    );
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "knowledge.change_begin",
            erased_begin_request.clone()
        )
        .await["error"]["code"],
        "knowledge_payload_erased"
    );
    let survivor = route(
        &mut client,
        "query",
        "knowledge.unit",
        json!({"unit_id":survivor_unit,"revision":1}),
    )
    .await;
    assert_eq!(survivor["document"], survivor_expected["document"]);
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    let repeated = tect_postgres::apply_knowledge_suppression_manifest(&pool, manifest, checkpoint)
        .await
        .unwrap();
    assert_eq!(repeated.entries_applied, 0);
    assert_eq!(repeated.remaining, 0);
    let after: i64 = sqlx::query_scalar(
        "SELECT generation FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(entry.tenant_id)
    .bind(entry.workspace_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(after, generation);
    pool.close().await;
    sqlx::query(&format!(
        "DROP DATABASE {} WITH (FORCE)",
        quoted_database(&database)
    ))
    .execute(&maintenance)
    .await
    .unwrap();
    maintenance.close().await;
    fs::remove_file(restore_pgpass).unwrap();
}
