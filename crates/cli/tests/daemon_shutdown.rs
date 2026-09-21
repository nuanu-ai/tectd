//! Real daemon signal acceptance. Uses only the isolated test database.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};

async fn start(runtime_url: &str, socket: &Path) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tectd"));
    command
        .env("TECT_DATABASE_URL", runtime_url)
        .env("TECT_DATABASE_MAX_CONNECTIONS", "2")
        .env("TECT_SOCKET", socket)
        .kill_on_drop(true);
    let mut child = command.spawn().unwrap();
    for _ in 0..100 {
        if socket.exists() {
            return child;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("daemon exited before binding: {status}");
        }
        sleep(Duration::from_millis(25)).await;
    }
    panic!("daemon did not bind its socket");
}

async fn terminate(mut child: Child, socket: &Path) {
    let pid = child.id().expect("running daemon has pid").to_string();
    let signal = Command::new("kill")
        .args(["-TERM", &pid])
        .status()
        .await
        .unwrap();
    assert!(signal.success());
    let status = timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("daemon handles SIGTERM promptly")
        .unwrap();
    assert!(status.success(), "daemon signal exit failed: {status}");
    assert!(!socket.exists(), "owned socket remains after SIGTERM");
}

#[tokio::test]
async fn sigterm_removes_only_owned_socket_and_allows_restart() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = sqlx::PgPool::connect(&admin_url).await.unwrap();
    tect_postgres::admin::migrate(&pool, &role).await.unwrap();
    pool.close().await;
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let socket = temp.path().canonicalize().unwrap().join("tectd.sock");

    terminate(start(&runtime_url, &socket).await, &socket).await;
    terminate(start(&runtime_url, &socket).await, &socket).await;
}
