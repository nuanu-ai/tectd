//! `TECT_JEV_NEW_WORKSPACE_DAILY_LIMIT` through real daemons against PostgreSQL: a
//! daemon told so enables Jev in the workspaces it creates and in no other, and it
//! does not start with a limit it cannot honour. Jev itself is never called here.
#[path = "advice_routes/daemon.rs"]
#[allow(dead_code)]
mod daemon;
#[allow(dead_code)]
mod recovery_support;

use daemon::{KEY, UNUSED_ENDPOINT, Urls, daemon, database, full_jev, route, serving};
use recovery_support::{Mcp, host_file, private_temp, tagged_url, tool_payload};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tect_postgres::admin;
use uuid::Uuid;

const LIMIT: &str = "TECT_JEV_NEW_WORKSPACE_DAILY_LIMIT";

/// One enrolled host; a workspace key names the same workspace for all its sessions.
async fn host(db: &Urls, root: &Path) -> PathBuf {
    let enrollment = admin::enroll_host(&db.pool, None, Vec::new())
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    config
}

/// A new native session of the host on `workspace`, opened through `socket`.
async fn session(config: &Path, socket: &Path, workspace: &str) -> Mcp {
    let mut client = Mcp::start(socket, config, &Uuid::new_v4().to_string(), workspace).await;
    client.call("open_workspace", json!({})).await;
    client
}

async fn setting(client: &mut Mcp) -> Value {
    let (response, raw) = route(client, "query", "workspace.advisory.config", json!({})).await;
    assert!(!raw.contains(KEY));
    tool_payload(&response)
}

fn shown(setting: &Value) -> (&Value, &Value, &Value) {
    (
        &setting["enabled"],
        &setting["daily_call_limit"],
        &setting["revision"],
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_daemon_told_so_enables_jev_in_the_workspaces_it_creates_and_no_other() {
    let db = database().await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let url = |name: &str| tagged_url(&db.runtime, &format!("tect-{name}-{}", Uuid::new_v4()));
    let name = |kind: &str| format!("advice-new-{kind}-{}", Uuid::new_v4().simple());
    let config = host(&db, &root).await;

    // Without the variable a new workspace has no setting, as before.
    let plain_socket = root.join("plain.sock");
    let mut plain = daemon(
        &url("new-workspace-plain"),
        &plain_socket,
        &full_jev(UNUSED_ENDPOINT),
        None,
    );
    serving(&mut plain, &plain_socket).await;
    let earlier = name("earlier");
    let mut client = session(&config, &plain_socket, &earlier).await;
    assert_eq!(
        shown(&setting(&mut client).await),
        (&json!(false), &Value::Null, &json!(0))
    );
    client.finish().await;

    // With it, the workspace this daemon creates is enabled from its first reply on.
    let mut told = full_jev(UNUSED_ENDPOINT).to_vec();
    told.push((LIMIT, "7"));
    let socket = root.join("told.sock");
    let mut child = daemon(&url("new-workspace-told"), &socket, &told, None);
    serving(&mut child, &socket).await;
    let created = name("created");
    let mut client = session(&config, &socket, &created).await;
    let enabled = setting(&mut client).await;
    assert_eq!(shown(&enabled), (&json!(true), &json!(7), &json!(1)));
    assert_eq!(enabled["calls_today"], 0);
    assert_eq!(enabled["provider_configured"], true);
    assert!(enabled["updated_by_session"].is_string(), "{enabled}");
    let journal: i64 = sqlx::query_scalar("SELECT count(*) FROM advice_journal")
        .fetch_one(&db.pool)
        .await
        .unwrap();

    // A second session of that workspace finds the same setting, not a second write.
    let mut again = session(&config, &socket, &created).await;
    assert_eq!(setting(&mut again).await, enabled);
    again.finish().await;

    // A workspace that existed before this daemon keeps having none.
    let mut old = session(&config, &socket, &earlier).await;
    assert_eq!(
        shown(&setting(&mut old).await),
        (&json!(false), &Value::Null, &json!(0))
    );
    old.finish().await;

    // It is an ordinary setting: the agent can switch it off, and the limit stays.
    let (response, _) = route(
        &mut client,
        "command",
        "workspace.advisory.configure",
        json!({"enabled":false,"expected_revision":1}),
    )
    .await;
    assert_eq!(
        shown(&tool_payload(&response)),
        (&json!(false), &json!(7), &json!(2))
    );
    // Enabling a workspace asks Jev nothing and journals nothing.
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM advice_journal")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(after, journal);
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_daemon_does_not_start_with_a_limit_it_cannot_honour() {
    let db = database().await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let client = full_jev(UNUSED_ENDPOINT);
    let with = |limit: &'static str| {
        let mut jev = client.to_vec();
        jev.push((LIMIT, limit));
        jev
    };
    // No provider to ask, a key alone, or a value that is not a whole number above zero.
    let refused: Vec<Vec<(&str, &str)>> = vec![
        vec![(LIMIT, "7")],
        vec![(LIMIT, "7"), ("TYPESAFE_API_KEY", KEY)],
        with(""),
        with("0"),
        with("-1"),
        with("many"),
        with("1.5"),
    ];
    for (index, jev) in refused.iter().enumerate() {
        let socket = root.join(format!("refused-{index}.sock"));
        let runtime = tagged_url(
            &db.runtime,
            &format!("tect-new-workspace-refused-{}", Uuid::new_v4()),
        );
        let mut child = daemon(&runtime, &socket, jev, None);
        let exit = tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(!exit.success(), "case {index} started");
        let stderr = fs::read_to_string(socket.with_extension("stderr")).unwrap();
        assert_eq!(stderr, "invalid_configuration\n", "case {index}");
        assert!(fs::symlink_metadata(&socket).is_err());
    }
}
