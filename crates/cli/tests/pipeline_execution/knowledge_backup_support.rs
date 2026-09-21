use super::recovery_support::{Daemon, Mcp};
use super::support::route;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::io::Write;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use uuid::Uuid;

fn database_url(url: &str, database: &str) -> String {
    let mut value = url::Url::parse(url).unwrap();
    value.set_path(&format!("/{database}"));
    value.into()
}

fn quoted_database(name: &str) -> String {
    assert!(
        name.bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    format!("\"{name}\"")
}

fn copy_bundle(source: &Path, destination: &Path) {
    std::fs::create_dir(destination).unwrap();
    std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o700)).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        let metadata = entry.metadata().unwrap();
        if metadata.is_dir() {
            copy_bundle(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
}

async fn database_exists(pool: &PgPool, database: &str) -> bool {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname=$1)")
        .bind(database)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn assert_private_database_creation(
    maintenance: &PgPool,
    database: &str,
    runtime_role: &str,
) {
    tect_postgres::admin::validate_restore_preflight(maintenance, runtime_role)
        .await
        .unwrap();
    tect_postgres::admin::create_restore_database(maintenance, database, runtime_role)
        .await
        .unwrap();
    let (allows_owner_connections, runtime_connect, public_connect): (bool, bool, bool) =
        sqlx::query_as(
            "SELECT d.datallowconn,
             has_database_privilege($1, d.datname, 'CONNECT'),
             EXISTS(SELECT 1 FROM aclexplode(COALESCE(d.datacl, acldefault('d', d.datdba))) a
                    WHERE a.grantee = 0 AND a.privilege_type = 'CONNECT')
             FROM pg_database d WHERE d.datname = $2",
        )
        .bind(runtime_role)
        .bind(database)
        .fetch_one(maintenance)
        .await
        .unwrap();
    assert!(allows_owner_connections);
    assert!(!runtime_connect);
    assert!(!public_connect);
    sqlx::query(&format!(
        "DROP DATABASE {} WITH (FORCE)",
        quoted_database(database)
    ))
    .execute(maintenance)
    .await
    .unwrap();
}

async fn restore_command(
    admin_url: &str,
    backup: &Path,
    database: &str,
    runtime_role: &str,
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tect-admin"))
        .env("TECT_ADMIN_DATABASE_URL", admin_url)
        .args(["restore", "--from"])
        .arg(backup)
        .args(["--database", database, "--runtime-role", runtime_role])
        .kill_on_drop(true)
        .output()
        .await
        .unwrap()
}

fn assert_no_connection_secret(output: &std::process::Output, admin_url: &str) {
    let streams = [output.stdout.as_slice(), output.stderr.as_slice()].concat();
    let text = String::from_utf8_lossy(&streams);
    assert!(!text.contains(admin_url));
    let parsed = url::Url::parse(admin_url).unwrap();
    if let Some(password) = parsed.password() {
        assert!(!text.contains(password));
        let decoded = percent_encoding::percent_decode_str(password)
            .decode_utf8()
            .unwrap();
        assert!(!text.contains(decoded.as_ref()));
    }
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
    let database = format!("tect_dk_restore_{}", Uuid::new_v4().simple());
    let backup = socket_dir.join("application-dk-backup");
    let backup_output = Command::new(env!("CARGO_BIN_EXE_tect-admin"))
        .env("TECT_ADMIN_DATABASE_URL", admin_url)
        .args(["backup", "--out"])
        .arg(&backup)
        .args(["--runtime-role", runtime_role])
        .kill_on_drop(true)
        .output()
        .await
        .unwrap();
    assert!(
        backup_output.status.success(),
        "production backup command failed: {}",
        String::from_utf8_lossy(&backup_output.stderr)
    );
    assert_no_connection_secret(&backup_output, admin_url);

    let maintenance_url = database_url(admin_url, "postgres");
    let maintenance = PgPool::connect(&maintenance_url).await.unwrap();
    let private_database = format!("tect_dk_private_{}", Uuid::new_v4().simple());
    assert_private_database_creation(&maintenance, &private_database, runtime_role).await;

    for (key, value) in [("dbname", "postgres"), ("sslpassword", "secret")] {
        let mut overridden = url::Url::parse(admin_url).unwrap();
        overridden.query_pairs_mut().append_pair(key, value);
        let rejected_database = format!("tect_dk_override_{}", Uuid::new_v4().simple());
        let rejected = restore_command(
            overridden.as_str(),
            &backup,
            &rejected_database,
            runtime_role,
        )
        .await;
        assert!(!rejected.status.success(), "accepted query override {key}");
        assert_no_connection_secret(&rejected, overridden.as_str());
        assert!(!database_exists(&maintenance, &rejected_database).await);
    }

    let missing_role = format!("missing_role_{}", Uuid::new_v4().simple());
    let missing_role_bundle = socket_dir.join("missing-role-backup");
    copy_bundle(&backup, &missing_role_bundle);
    let manifest_path = missing_role_bundle.join("manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["runtime_role"] = Value::String(missing_role.clone());
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let missing_role_database = format!("tect_dk_missing_{}", Uuid::new_v4().simple());
    let missing_role_output = restore_command(
        admin_url,
        &missing_role_bundle,
        &missing_role_database,
        &missing_role,
    )
    .await;
    assert!(!missing_role_output.status.success());
    assert_no_connection_secret(&missing_role_output, admin_url);
    assert!(!database_exists(&maintenance, &missing_role_database).await);

    let restore_output = restore_command(admin_url, &backup, &database, runtime_role).await;
    assert!(
        restore_output.status.success(),
        "production restore command failed: {}",
        String::from_utf8_lossy(&restore_output.stderr)
    );
    assert_no_connection_secret(&restore_output, admin_url);

    let restored_admin_url = database_url(admin_url, &database);
    let restored_pool = PgPool::connect(&restored_admin_url).await.unwrap();
    let (runtime_connect, public_connect): (bool, bool) = sqlx::query_as(
        "SELECT has_database_privilege($1, current_database(), 'CONNECT'), EXISTS(
         SELECT 1 FROM pg_database d CROSS JOIN LATERAL aclexplode(
         COALESCE(d.datacl, acldefault('d', d.datdba))) a
         WHERE d.datname = current_database() AND a.grantee = 0 AND a.privilege_type = 'CONNECT')",
    )
    .bind(runtime_role)
    .fetch_one(&restored_pool)
    .await
    .unwrap();
    assert!(runtime_connect);
    assert!(!public_connect);
    restored_pool.close().await;

    let existing = restore_command(admin_url, &backup, &database, runtime_role).await;
    assert!(!existing.status.success());
    assert_no_connection_secret(&existing, admin_url);
    assert!(database_exists(&maintenance, &database).await);

    let corrupt = socket_dir.join("corrupt-backup");
    copy_bundle(&backup, &corrupt);
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(corrupt.join("manifest.json")).unwrap()).unwrap();
    let graph_file = manifest["graphs"][0]["file"].as_str().unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(corrupt.join(graph_file))
        .unwrap()
        .write_all(b"corrupt")
        .unwrap();
    let corrupt_database = format!("tect_dk_corrupt_{}", Uuid::new_v4().simple());
    let corrupt_output =
        restore_command(admin_url, &corrupt, &corrupt_database, runtime_role).await;
    assert!(!corrupt_output.status.success());
    assert_no_connection_secret(&corrupt_output, admin_url);
    assert!(!database_exists(&maintenance, &corrupt_database).await);

    let malformed = socket_dir.join("malformed-backup");
    copy_bundle(&backup, &malformed);
    std::fs::write(malformed.join("manifest.json"), b"{").unwrap();
    let malformed_database = format!("tect_dk_malformed_{}", Uuid::new_v4().simple());
    let malformed_output =
        restore_command(admin_url, &malformed, &malformed_database, runtime_role).await;
    assert!(!malformed_output.status.success());
    assert_no_connection_secret(&malformed_output, admin_url);
    assert!(!database_exists(&maintenance, &malformed_database).await);

    let malformed_digest = socket_dir.join("malformed-digest-backup");
    copy_bundle(&backup, &malformed_digest);
    let manifest_path = malformed_digest.join("manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["graphs"][0]["native_digest"] = Value::String("ABC123".into());
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let malformed_digest_database = format!("tect_dk_digest_{}", Uuid::new_v4().simple());
    let malformed_digest_output = restore_command(
        admin_url,
        &malformed_digest,
        &malformed_digest_database,
        runtime_role,
    )
    .await;
    assert!(!malformed_digest_output.status.success());
    assert_no_connection_secret(&malformed_digest_output, admin_url);
    assert!(!database_exists(&maintenance, &malformed_digest_database).await);

    let linked = socket_dir.join("linked-backup");
    copy_bundle(&backup, &linked);
    let moved_manifest: PathBuf = socket_dir.join("linked-manifest.json");
    std::fs::rename(linked.join("manifest.json"), &moved_manifest).unwrap();
    symlink(&moved_manifest, linked.join("manifest.json")).unwrap();
    let linked_database = format!("tect_dk_linked_{}", Uuid::new_v4().simple());
    let linked_output = restore_command(admin_url, &linked, &linked_database, runtime_role).await;
    assert!(!linked_output.status.success());
    assert_no_connection_secret(&linked_output, admin_url);
    assert!(!database_exists(&maintenance, &linked_database).await);

    let socket = socket_dir.join("restored-knowledge.sock");
    let restored_runtime_url = database_url(runtime_url, &database);
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
    let source_database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(source_pool)
        .await
        .unwrap();
    assert_ne!(source_database, database);
}
