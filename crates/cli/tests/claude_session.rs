//! Disposable stdio -> Unix daemon -> PostgreSQL fixture, not native Claude acceptance.
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{io::Write, os::unix::fs::PermissionsExt, process::Stdio, sync::Arc};
use tect_application::WorkspaceService;
use tect_postgres::{PgStore, admin};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use uuid::Uuid;

async fn exchange(child: &mut tokio::process::Child, message: Value) -> Value {
    let input = child.stdin.as_mut().unwrap();
    input
        .write_all(format!("{message}\n").as_bytes())
        .await
        .unwrap();
    input.flush().await.unwrap();
    let mut line = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        BufReader::new(child.stdout.as_mut().unwrap()).read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    serde_json::from_str(&line).unwrap()
}

fn payload(response: &Value) -> Value {
    assert!(response.get("error").is_none(), "{response}");
    serde_json::from_str(response["result"]["content"][1]["text"].as_str().unwrap()).unwrap()
}

fn attest(
    directory: &std::path::Path,
    host_id: Uuid,
    native_id: &str,
    call_id: &str,
    tool: &str,
    input: &Value,
) {
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_tectd-mcp"))
        .args([
            "claude-pre-tool-use",
            "--destination-host-id",
            &host_id.to_string(),
            "--server-alias",
            "tectd",
            "--context-dir",
        ])
        .arg(directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            &serde_json::to_vec(&json!({
                "hook_event_name":"PreToolUse", "session_id":native_id,
                "tool_use_id":call_id, "tool_name":format!("mcp__tectd__{tool}"), "tool_input":input
            }))
            .unwrap(),
        )
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn attested_claude_session_survives_stdio_restart_and_stays_isolated() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let enrollment = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let store = Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap());
    let service = Arc::new(WorkspaceService::new(
        store,
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    ));
    let temp = tempfile::tempdir().unwrap();
    let private = temp.path().canonicalize().unwrap();
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = private.join("d.sock");
    let config = private.join("host.json");
    std::fs::write(&config, serde_json::to_vec(&enrollment.auth).unwrap()).unwrap();
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).unwrap();
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let daemon = tokio::spawn(tect_host::serve(listener, service));
    let original = Uuid::new_v4().to_string();
    let distinct = Uuid::new_v4().to_string();
    let mut first_session = None;
    for (index, native_id) in [&original, &original, &distinct].into_iter().enumerate() {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_tectd-mcp"))
            .env("TECT_SOCKET", &socket)
            .env("TECT_HOST_CONFIG", &config)
            .env("TECT_WORKSPACE_KEY", "claude-stdio-fixture")
            .env("TECT_NATIVE_IDENTITY_PROVIDER", "claude_pre_tool_use")
            .env("TECT_CLAUDE_ATTESTATION_DIR", &private)
            .env("TECT_CLAUDE_MCP_SERVER_ALIAS", "tectd")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let initialized = exchange(&mut child, json!({
            "jsonrpc":"2.0", "id":1, "method":"initialize", "params":{
                "protocolVersion":"2025-11-25", "capabilities":{},
                "clientInfo":{"name":"claude-fixture", "version":"1", "title":"Claude Code",
                    "description":"Fixture", "websiteUrl":"https://code.claude.com",
                    "icons":[{"src":"https://example.invalid/icon.png", "mimeType":"image/png", "sizes":["64x64"], "theme":"dark"}]}
            }
        })).await;
        assert!(initialized.get("error").is_none(), "{initialized}");
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .await
            .unwrap();
        let missing = exchange(&mut child, json!({"jsonrpc":"2.0", "id":2, "method":"tools/call",
            "params":{"name":"get_state", "arguments":{}, "_meta":{"threadId":native_id,"claudecode/toolUseId":"missing"}}})).await;
        assert_eq!(payload(&missing)["error"]["code"], "invalid_native_session");
        let call_id = format!("open-{index}");
        let arguments = json!({"route":"workspace.open", "params":{}});
        attest(
            &private,
            enrollment.auth.host_id,
            native_id,
            &call_id,
            "command",
            &arguments,
        );
        let mismatch = exchange(&mut child, json!({"jsonrpc":"2.0", "id":3, "method":"tools/call",
            "params":{"name":"get_state", "arguments":{}, "_meta":{"claudecode/toolUseId":call_id}}})).await;
        assert_eq!(
            payload(&mismatch)["error"]["code"],
            "invalid_native_session"
        );
        let opened = exchange(&mut child, json!({"jsonrpc":"2.0", "id":4, "method":"tools/call",
            "params":{"name":"command", "arguments":arguments, "_meta":{"claudecode/toolUseId":call_id}}})).await;
        let opened = payload(&opened);
        assert_eq!(opened["status"], "ready", "{opened}");
        assert_eq!(opened["session"]["native_session_id"], *native_id);
        if index == 0 {
            first_session = Some(opened["session"]["id"].clone());
        } else if index == 1 {
            assert_eq!(first_session.as_ref().unwrap(), &opened["session"]["id"]);
        } else {
            assert_ne!(first_session.as_ref().unwrap(), &opened["session"]["id"]);
        }
        println!(
            "claude_fixture_session={}",
            json!({"process":index, "native_session_id":native_id, "session_id":opened["session"]["id"]})
        );
        drop(child.stdin.take());
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_sessions WHERE host_id=$1")
        .bind(enrollment.auth.host_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2, "rejected calls must not create sessions");
    daemon.abort();
    let _ = daemon.await;
}
