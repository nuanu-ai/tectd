//! Matrix transport configuration is checked before PostgreSQL is contacted.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn partial_matrix_tuple_rejects_daemon_startup_without_database() {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let socket = temp.path().canonicalize().unwrap().join("tectd.sock");
    let output = Command::new(env!("CARGO_BIN_EXE_tectd"))
        .env_clear()
        .env("TECT_SOCKET", &socket)
        .env(
            "TECT_DATABASE_URL",
            "postgres://unused:unused@127.0.0.1:1/unused",
        )
        .env(
            "TECT_JEV_MATRIX_ENDPOINT",
            "https://example.com/v1/systemone",
        )
        .env("TYPESAFE_API_KEY", "fixture-key")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "invalid_configuration"
    );
    assert!(!socket.exists());
}
