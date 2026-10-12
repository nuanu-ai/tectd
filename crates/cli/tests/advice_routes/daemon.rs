//! An owned daemon with an exact Jev environment, and one route call through the bridge.
use crate::recovery_support::Mcp;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{
    fs,
    os::unix::fs::{FileTypeExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    process::Stdio,
    time::Duration,
};
use tect_postgres::admin;
use tokio::process::{Child, Command};

pub const KEY: &str = "route-test-key-0123456789";
pub const UNUSED_ENDPOINT: &str = "http://127.0.0.1:9/v1/systemone";

pub struct Urls {
    pub pool: PgPool,
    pub runtime: String,
}

pub async fn database() -> Urls {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime = std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    Urls { pool, runtime }
}

/// The daemon with exactly the given Jev environment, whatever the test runner has.
pub fn daemon(url: &str, socket: &Path, jev: &[(&str, &str)], path: Option<&str>) -> Child {
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(socket.with_extension("stderr"))
        .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_tectd"));
    command
        .env("TECT_DATABASE_URL", url)
        .env("TECT_SOCKET", socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .kill_on_drop(true);
    for name in tect_host::JEV_ENVIRONMENT {
        command.env_remove(name);
    }
    for (name, value) in jev {
        command.env(name, value);
    }
    if let Some(path) = path {
        command.env("PATH", path);
    }
    command.spawn().unwrap()
}

pub async fn serving(child: &mut Child, socket: &Path) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            assert!(
                child.try_wait().unwrap().is_none(),
                "owned daemon exited during startup"
            );
            if fs::symlink_metadata(socket)
                .is_ok_and(|m| m.file_type().is_socket() && m.permissions().mode() & 0o777 == 0o600)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

pub async fn route(client: &mut Mcp, tool: &str, route: &str, params: Value) -> (Value, String) {
    let response = client
        .exchange(
            "tools/call",
            json!({"name":tool,"arguments":{"route":route,"params":params}}),
        )
        .await;
    let raw = response.to_string();
    (response, raw)
}

pub fn full_jev(endpoint: &str) -> [(&str, &str); 4] {
    [
        ("TECT_JEV_ENDPOINT", endpoint),
        ("TECT_JEV_MODEL", "jev-latest"),
        ("TECT_JEV_TIMEOUT_MS", "2000"),
        ("TYPESAFE_API_KEY", KEY),
    ]
}
