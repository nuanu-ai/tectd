use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::{
    env, fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
use tokio::process::Command;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct GraphBackup {
    pub(super) iri: String,
    pub(super) digest: String,
    pub(super) file: String,
}

pub struct ApplicationBackup {
    pub(super) dump: PathBuf,
    pub(super) graph_dir: PathBuf,
}

pub(super) fn database_url(url: &str, database: &str) -> String {
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

pub(super) fn quoted_database(name: &str) -> String {
    assert!(
        name.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    );
    format!("\"{name}\"")
}

pub(super) fn restore_pgpass(directory: &Path, database: &str) -> PathBuf {
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

pub(super) fn pgpass_password(path: &Path, user: &str) -> String {
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

pub(super) fn authenticated_url(url: &str, password: &str) -> String {
    let password = password.bytes().fold(String::new(), |mut value, byte| {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            value.push(char::from(byte));
        } else {
            value.push_str(&format!("%{byte:02X}"));
        }
        value
    });
    let user_start = url.find("://").expect("database URL scheme") + 3;
    let user_end = url[user_start..]
        .find('@')
        .map(|offset| user_start + offset)
        .expect("database URL user separator");
    let user = url[user_start..user_end]
        .split_once(':')
        .map_or(&url[user_start..user_end], |(user, _)| user);
    format!(
        "{}{user}:{password}{}",
        &url[..user_start],
        &url[user_end..]
    )
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
