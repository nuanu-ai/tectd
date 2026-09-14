use super::recovery_support::{Daemon, Mcp};
use super::support::{route, route_error};
#[path = "knowledge_maintenance_restore_support.rs"]
mod knowledge_maintenance_restore_support;
#[path = "knowledge_search_restore_support.rs"]
mod knowledge_search_restore_support;
use serde_json::{Value, json};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{env, fs, path::Path, process::Stdio, time::Duration};
use tect_domain::KnowledgeEmbeddingJobCompletion;
use tect_postgres::{KnowledgeSuppressionCheckpoint, KnowledgeSuppressionManifest};
use tokio::process::Command;
use uuid::Uuid;

#[path = "knowledge_suppression_backup_capture_support.rs"]
mod knowledge_suppression_backup_capture_support;
pub use knowledge_suppression_backup_capture_support::{ApplicationBackup, capture};
use knowledge_suppression_backup_capture_support::{
    GraphBackup, authenticated_url, database_url, pgpass_password, quoted_database, restore_pgpass,
};

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
    target_maintenance_task: Uuid,
    survivor_maintenance_task: Uuid,
    survivor_maintenance_revision: i64,
    restored_maintenance_lease: Uuid,
    planning_program: &Value,
    planning_begin_request: &Value,
    planning_refresh_request: &Value,
    planning_marker: &str,
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
    let protected_planning: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM planning_knowledge_manifests \
           WHERE owner_id=$1 AND pg_catalog.jsonb_array_length(selected)>0), \
         (SELECT count(*) FROM knowledge_owned_copies \
           WHERE relation_name='programs' AND row_id=$1 AND NOT redacted)",
    )
    .bind(Uuid::parse_str(planning_program.as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(protected_planning.0 > 0 && protected_planning.1 > 0);
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
    knowledge_maintenance_restore_support::verify_blocked(
        &pool,
        target_maintenance_task,
        survivor_maintenance_task,
        restored_maintenance_lease,
    )
    .await;
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
    let entry = manifest
        .entries
        .iter()
        .find(|entry| entry.unit_id.to_string() == erased_unit.as_str().unwrap())
        .unwrap();
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
    knowledge_maintenance_restore_support::verify_requalified(
        &pool,
        &runtime,
        config,
        workspace,
        entry.unit_id,
        target_maintenance_task,
        Uuid::parse_str(survivor_unit.as_str().unwrap()).unwrap(),
        survivor_maintenance_task,
        survivor_maintenance_revision,
        restored_maintenance_lease,
    )
    .await;
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
            "query",
            "program.get",
            json!({"program_id":planning_program})
        )
        .await["error"]["code"],
        "knowledge_payload_erased"
    );
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "program.begin",
            planning_begin_request.clone()
        )
        .await["error"]["code"],
        "knowledge_payload_erased"
    );
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "program.knowledge.refresh",
            planning_refresh_request.clone()
        )
        .await["error"]["code"],
        "knowledge_payload_erased"
    );
    let listed = route(&mut client, "query", "program.list", json!({"limit":100})).await;
    assert!(!listed.to_string().contains(planning_marker));
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
