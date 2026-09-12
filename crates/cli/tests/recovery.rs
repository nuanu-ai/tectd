//! Real process death, observed in-flight DB rollback and committed reply loss.
mod recovery_support;
use recovery_support::{
    Daemon, Mcp, host_file, private_temp, public_call, tagged_url, tool_payload,
};
use serde_json::json;
use sqlx::PgPool;
use std::time::Duration;
use tect_postgres::admin;
use uuid::Uuid;

fn without_actions(mut payload: serde_json::Value) -> serde_json::Value {
    let object = payload.as_object_mut().unwrap();
    object.remove("actions");
    object.remove("recommended_action");
    payload
}

async fn counts(pool: &PgPool, tenant: Uuid) -> Vec<i64> {
    let mut result = Vec::new();
    for table in [
        "workspaces",
        "memberships",
        "agent_sessions",
        "workspace_events",
    ] {
        result.push(
            sqlx::query_scalar::<_, i64>(&format!(
                "SELECT count(*) FROM {table} WHERE tenant_id=$1"
            ))
            .bind(tenant)
            .fetch_one(pool)
            .await
            .unwrap(),
        );
    }
    result
}
async fn wait_for_session(pool: &PgPool, host_id: Uuid, native: &str) -> Uuid {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let id: Option<Uuid> = sqlx::query_scalar(
                "SELECT id FROM agent_sessions WHERE host_id=$1 AND native_session_id=$2",
            )
            .bind(host_id)
            .bind(native)
            .fetch_optional(pool)
            .await
            .unwrap();
            if let Some(id) = id {
                return id;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("bootstrap should have committed before discarding its reply")
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_daemon_crash_rolls_back_and_lost_reply_recovers_committed_identity() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let tag = format!("tect-crash-{}", Uuid::new_v4());
    let tagged_runtime = tagged_url(&runtime_url, &tag);
    let socket = root.join("daemon.sock");
    let mut daemon = Daemon::start(&tagged_runtime, socket.clone()).await;
    let host = admin::enroll_host(&pool, None, vec![root.to_str().unwrap().into()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &host.auth);
    let native = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config, &native, "persistent-fixture").await;
    let initial = client.call("open_workspace", json!({})).await;
    let repo = root.join("source");
    std::fs::create_dir(&repo).unwrap();
    assert!(
        tokio::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(&repo)
            .status()
            .await
            .unwrap()
            .success()
    );
    let source = client.call("register_source", json!({"path":repo})).await;
    let persisted = client
        .call("select_worktrees", json!({"worktree_ids":[source["id"]]}))
        .await;
    assert_eq!(persisted["workspace"], initial["workspace"]);
    client.finish().await;

    let interrupted = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let interrupted_config = root.join("interrupted.json");
    host_file(&interrupted_config, &interrupted.auth);
    let interrupted_native = Uuid::new_v4().to_string();
    let mut interrupted_client = Mcp::start(
        &socket,
        &interrupted_config,
        &interrupted_native,
        "interrupted-bootstrap",
    )
    .await;
    // This permits the initial session SELECT but blocks its later INSERT, after
    // the actual application has prepared workspace and membership in its transaction.
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE agent_sessions IN SHARE MODE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    interrupted_client
        .send("tools/call", public_call("open_workspace", json!({})))
        .await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND state='active' AND wait_event_type='Lock' AND query LIKE 'INSERT INTO agent_sessions%')").bind(&tag).fetch_one(&pool).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("real runtime session INSERT must be blocked before commit");
    daemon.crash().await;
    blocker.rollback().await.unwrap();
    assert_eq!(counts(&pool, interrupted.tenant_id).await, [0, 0, 0, 0]);
    interrupted_client.kill().await;
    // Product refuses an existing stale socket; only our operator cleanup follows.
    let refused = tokio::process::Command::new(env!("CARGO_BIN_EXE_tectd"))
        .env("TECT_DATABASE_URL", &tagged_runtime)
        .env("TECT_SOCKET", &socket)
        .output()
        .await
        .unwrap();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("invalid_configuration"));
    daemon.remove_owned_stale_socket();
    let mut daemon = Daemon::start(&tagged_runtime, socket.clone()).await;
    let mut client = Mcp::start(&socket, &config, &native, "persistent-fixture").await;
    assert_eq!(
        without_actions(client.call("get_state", json!({})).await),
        without_actions(persisted.clone())
    );
    assert_eq!(
        without_actions(client.call("open_workspace", json!({})).await),
        without_actions(persisted.clone())
    );
    client.finish().await;
    let mut retried = Mcp::start(
        &socket,
        &interrupted_config,
        &interrupted_native,
        "interrupted-bootstrap",
    )
    .await;
    let recovered = retried.call("open_workspace", json!({})).await;
    assert_eq!(
        recovered["session"]["native_session_id"],
        interrupted_native
    );
    assert_eq!(counts(&pool, interrupted.tenant_id).await, [1, 1, 1, 2]);
    retried.finish().await;

    // A response can be lost after the transaction committed. Observe the commit
    // independently, discard the bridge without reading its response, then retry.
    let lost = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let lost_config = root.join("lost.json");
    host_file(&lost_config, &lost.auth);
    let lost_native = Uuid::new_v4().to_string();
    let mut lost_client = Mcp::start(&socket, &lost_config, &lost_native, "lost-reply").await;
    lost_client
        .send("tools/call", public_call("open_workspace", json!({})))
        .await;
    let committed_id = wait_for_session(&pool, lost.auth.host_id, &lost_native).await;
    lost_client.kill().await;
    let before = counts(&pool, lost.tenant_id).await;
    assert_eq!(before, [1, 1, 1, 2]);
    let mut retry = Mcp::start(&socket, &lost_config, &lost_native, "lost-reply").await;
    let resumed = retry.call("open_workspace", json!({})).await;
    assert_eq!(resumed["session"]["id"], committed_id.to_string());
    assert_eq!(counts(&pool, lost.tenant_id).await, before);
    // Revocation is observable through real MCP; no operator commands are tools.
    admin::revoke_session(&pool, committed_id).await.unwrap();
    let denied = retry
        .exchange("tools/call", public_call("open_workspace", json!({})))
        .await;
    assert_eq!(denied["result"]["isError"], true);
    let denied = tool_payload(&denied);
    assert_eq!(denied["error"]["code"], "session_revoked");
    let listed = retry.exchange("tools/list", json!({})).await;
    let names: Vec<_> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["get_state", "query", "command", "execute", "help"]);
    assert!(names.iter().all(|n| !n.starts_with("revoke")));
    retry.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
