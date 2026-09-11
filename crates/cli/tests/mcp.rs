//! Actual stdio -> Unix daemon -> PostgreSQL with explicit synthetic fixture IDs.
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    process::Stdio,
    sync::Arc,
};
use tect_application::WorkspaceService;
use tect_domain::{HostAuth, RequestContext, StateStatus};
use tect_postgres::{PgStore, admin};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use uuid::Uuid;

fn payload(response: &Value) -> Value {
    assert!(response.get("error").is_none(), "{response}");
    assert!(
        response["result"].get("structuredContent").is_none(),
        "{response}"
    );
    let content = response["result"]["content"].as_array().unwrap();
    assert_eq!(content.len(), 2, "{response}");
    assert_eq!(content[0]["type"], "text");
    let intro = content[0]["text"].as_str().unwrap();
    assert!(!intro.is_empty() && intro.len() <= 2_000);
    assert_eq!(content[1]["type"], "text");
    serde_json::from_str(content[1]["text"].as_str().unwrap()).unwrap()
}

async fn exchange(
    input: &mut tokio::process::ChildStdin,
    output: &mut BufReader<tokio::process::ChildStdout>,
    message: Value,
) -> Value {
    input
        .write_all(format!("{message}\n").as_bytes())
        .await
        .unwrap();
    input.flush().await.unwrap();
    let mut line = String::new();
    let size = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        output.read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(size > 0, "MCP bridge exited without a response");
    serde_json::from_str(&line).unwrap()
}

fn config_file(path: &std::path::Path, auth: &HostAuth) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec(auth).unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_mcp_schema_rejects_identity_override_and_recovers_session() {
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
    ));
    let temp = tempfile::tempdir().unwrap();
    let private_path = temp.path().canonicalize().unwrap();
    std::fs::set_permissions(&private_path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = private_path.join("d.sock");
    let config = private_path.join("host.json");
    config_file(&config, &enrollment.auth);
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let daemon = tokio::spawn(tect_host::serve(listener, service));
    let native_id = Uuid::new_v4().to_string();
    let mut first_session = None;

    for reconnect in 0..2 {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_tectd-mcp"))
            .env("TECT_SOCKET", &socket)
            .env("TECT_HOST_CONFIG", &config)
            .env("TECT_WORKSPACE_KEY", "stdio-fixture")
            .env_remove("CODEX_SESSION_ID")
            .env_remove("CODEX_THREAD_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let init = exchange(
            &mut input,
            &mut output,
            json!({
                "jsonrpc":"2.0","id":1,"method":"initialize","params":{
                    "protocolVersion":"2025-06-18","capabilities":{},
                    "clientInfo":{"name":"tect-product-test","version":"1"}
                }
            }),
        )
        .await;
        assert!(init.get("error").is_none(), "{init}");
        assert_eq!(init["result"]["serverInfo"]["name"], "tectd-mcp");
        assert_eq!(init["result"]["serverInfo"]["title"], "TectD MCP");
        assert!(!init.to_string().contains(&enrollment.auth.credential));
        input
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .await
            .unwrap();
        let listed = exchange(
            &mut input,
            &mut output,
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        )
        .await;
        let tools = listed["result"]["tools"].as_array().unwrap();
        let state_tool = tools.iter().find(|t| t["name"] == "get_state").unwrap();
        assert_eq!(state_tool["annotations"]["readOnlyHint"], true);
        assert_eq!(state_tool["inputSchema"]["additionalProperties"], false);
        let state = exchange(
            &mut input,
            &mut output,
            json!({
                "jsonrpc":"2.0","id":3,"method":"tools/call",
                "params":{"name":"get_state","arguments":{},"_meta":{"threadId":native_id}}
            }),
        )
        .await;
        assert_eq!(
            payload(&state)["status"],
            if reconnect == 0 {
                "uninitialized"
            } else {
                "ready"
            },
            "{state}"
        );

        let spoof = exchange(
            &mut input,
            &mut output,
            json!({
                "jsonrpc":"2.0","id":4,"method":"tools/call",
                "params":{"name":"open_workspace","arguments":{"workspace_key":"spoofed"},"_meta":{"threadId":native_id}}
            }),
        )
        .await;
        assert!(spoof.get("error").is_some() || spoof["result"]["isError"] == true);
        let state = exchange(
            &mut input,
            &mut output,
            json!({
                "jsonrpc":"2.0","id":5,"method":"tools/call",
                "params":{"name":"open_workspace","arguments":{},"_meta":{"threadId":native_id}}
            }),
        )
        .await;
        let content = payload(&state);
        assert_eq!(content["status"], "ready", "{state}");
        assert_eq!(content["session"]["native_session_id"], native_id);
        if let Some(id) = &first_session {
            assert_eq!(id, &content["session"]["id"]);
        }
        first_session = Some(content["session"]["id"].clone());
        assert!(!state.to_string().contains(&enrollment.auth.credential));
        drop(input);
        let exit = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(exit.success());
    }

    // The daemon repeats schema checks: a direct caller cannot bypass the bridge.
    let context = RequestContext {
        auth: enrollment.auth,
        native_session_id: native_id,
        workspace_key: "stdio-fixture".into(),
    };
    assert!(
        tect_host::call(
            &socket,
            &context,
            "get_state",
            json!({"tenant_id":Uuid::new_v4()})
        )
        .await
        .is_err()
    );
    let state = tect_host::call(&socket, &context, "get_state", json!({}))
        .await
        .unwrap();
    assert_eq!(state.status, StateStatus::Ready);
    let wrong: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workspaces WHERE tenant_id=$1 AND key='spoofed'")
            .bind(enrollment.tenant_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(wrong, 0);
    daemon.abort();
    let _ = daemon.await;
}
