//! Real PostgreSQL and stdio-MCP recovery acceptance for workspace setup publication.

mod recovery_support;

use recovery_support::{
    Daemon, Mcp, action_name, host_file, private_temp, public_call, ready_action, tagged_url,
    tool_payload,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::Duration;
use tect_postgres::admin;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
struct SetupRow {
    status: String,
    revision: i64,
    content: Option<String>,
    current_step: String,
    applied_from_revision: Option<i64>,
    applied_sha256: Option<String>,
}

#[derive(Clone, Copy)]
struct ReadySetup {
    id: Uuid,
    revision: i64,
}

async fn row(pool: &PgPool, tenant_id: Uuid, setup_id: Uuid) -> SetupRow {
    sqlx::query_as(
        "SELECT status, revision, content, current_step, applied_from_revision, applied_sha256 \
         FROM workspace_setups WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant_id)
    .bind(setup_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn assert_single_applied(
    pool: &PgPool,
    tenant_id: Uuid,
    ready: ReadySetup,
    content: &str,
) -> SetupRow {
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workspace_setups WHERE tenant_id=$1 AND id=$2")
            .bind(tenant_id)
            .bind(ready.id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    let applied = row(pool, tenant_id, ready.id).await;
    assert_eq!(applied.status, "applied");
    assert_eq!(applied.current_step, "complete");
    assert_eq!(applied.revision, ready.revision + 1);
    assert_eq!(applied.applied_from_revision, Some(ready.revision));
    assert_eq!(applied.content.as_deref(), Some(content));
    assert!(
        applied
            .applied_sha256
            .as_ref()
            .is_some_and(|v| v.len() == 64)
    );
    applied
}

async fn open_and_bind(client: &mut Mcp, directory: &Path) {
    let state = client.call("open_workspace", json!({})).await;
    assert_eq!(state["status"], "ready");
    assert!(state["workspace"].is_object());
    assert!(state["session"].is_object());
    let inspected = client
        .call(
            "inspect_setup",
            json!({"task_directory":directory.to_str().unwrap()}),
        )
        .await;
    assert_eq!(inspected["file"]["status"], "missing");
    assert_eq!(inspected["file"]["observed_now"], true);
}

async fn ready_setup(client: &mut Mcp, directory: &Path, content: &str) -> ReadySetup {
    open_and_bind(client, directory).await;
    let begun = client
        .call(
            "begin_setup",
            json!({"request_id":Uuid::new_v4(),"input":"Create bounded workspace instructions for this isolated acceptance fixture."}),
        )
        .await;
    let id: Uuid = begun["setup"]["id"].as_str().unwrap().parse().unwrap();
    let revision = begun["setup"]["revision"].as_i64().unwrap();
    let saved = client
        .call(
            "save_setup",
            json!({"setup_id":id,"revision":revision,"input_cursor":1,
                "ready":true,"content":content,"working_notes":"acceptance fixture complete"}),
        )
        .await;
    assert_eq!(saved["setup"]["status"], "draft");
    assert_eq!(saved["setup"]["current_step"], "ready_to_apply");
    assert_eq!(saved["setup"]["content"], content);
    ReadySetup {
        id,
        revision: saved["setup"]["revision"].as_i64().unwrap(),
    }
}

fn apply_args(ready: ReadySetup) -> Value {
    json!({"setup_id":ready.id,"revision":ready.revision})
}

fn assert_apply_retry(payload: &Value, ready: ReadySetup) {
    assert_eq!(payload["recommended_action"], 0);
    assert_eq!(
        payload["actions"][0],
        ready_action("apply_setup", apply_args(ready))
    );
    assert_program_navigation(payload);
}

fn assert_reload(payload: &Value, ready: ReadySetup) {
    assert_eq!(payload["recommended_action"], 0);
    assert_eq!(
        payload["actions"][0],
        ready_action(
            "get_setup",
            json!({"setup_id":ready.id,"after_input":0,"limit":25}),
        )
    );
    assert_program_navigation(payload);
}

fn assert_program_navigation(payload: &Value) {
    assert_eq!(payload["actions"].as_array().unwrap().len(), 3);
    assert_eq!(
        payload["actions"][1],
        ready_action("list_programs", json!({"limit":25}))
    );
    assert_eq!(action_name(&payload["actions"][2]), Some("program.begin"));
}

async fn install_owned_failure(pool: &PgPool, setup_id: Uuid) -> (String, String) {
    let suffix = setup_id.simple().to_string();
    let function = format!("tect_test_setup_failure_{suffix}");
    let trigger = format!("tect_test_setup_trigger_{suffix}");
    sqlx::query(&format!(
        "CREATE FUNCTION public.{function}() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN RAISE EXCEPTION 'owned setup recovery fixture'; END $$"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "CREATE TRIGGER {trigger} BEFORE UPDATE OF status ON public.workspace_setups \
         FOR EACH ROW WHEN (OLD.status='draft' AND NEW.status='applied' \
         AND NEW.id='{setup_id}'::uuid) EXECUTE FUNCTION public.{function}()"
    ))
    .execute(pool)
    .await
    .unwrap();
    (trigger, function)
}

async fn remove_owned_failure(pool: &PgPool, trigger: &str, function: &str) {
    sqlx::query(&format!(
        "DROP TRIGGER {trigger} ON public.workspace_setups"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(&format!("DROP FUNCTION public.{function}()"))
        .execute(pool)
        .await
        .unwrap();
}

async fn wait_until_blocked(pool: &PgPool, application_name: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 \
                 AND state='active' AND cardinality(pg_blocking_pids(pid)) > 0 \
                 AND query LIKE '%workspace_setups%')",
            )
            .bind(application_name)
            .fetch_one(pool)
            .await
            .unwrap();
            if blocked {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the real apply request must reach its scoped row lock");
}

async fn wait_until_applied(pool: &PgPool, tenant_id: Uuid, setup_id: Uuid) -> SetupRow {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let current = row(pool, tenant_id, setup_id).await;
            if current.status == "applied" {
                return current;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("canonical setup row did not reach applied")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn setup_publication_recovers_across_database_and_stdio_failures() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let tag = format!("tect-setup-recovery-{}", Uuid::new_v4());
    let tagged_runtime = tagged_url(&runtime_url, &tag);
    let socket = root.join("daemon.sock");
    let mut daemon = Daemon::start(&tagged_runtime, socket.clone()).await;
    let host = admin::enroll_host_with_grants(
        &pool,
        None,
        Vec::new(),
        vec![root.to_str().unwrap().to_owned()],
    )
    .await
    .unwrap();
    let config = root.join("host.json");
    host_file(&config, &host.auth);
    let workspace_key = format!("setup-recovery-{}", Uuid::new_v4());

    // Publication survives a later DB failure; the durable ready revision authorizes recovery.
    let db_directory = root.join("database-failure");
    fs::create_dir(&db_directory).unwrap();
    let db_content = "# Database recovery\n\nPreserve this exact durable draft.\n";
    let mut db_client = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    let db_ready = ready_setup(&mut db_client, &db_directory, db_content).await;
    let before = row(&pool, host.tenant_id, db_ready.id).await;
    assert_eq!(before.status, "draft");
    assert_eq!(before.current_step, "ready_to_apply");
    assert_eq!(before.revision, db_ready.revision);
    assert_eq!(before.content.as_deref(), Some(db_content));
    assert_eq!(before.applied_from_revision, None);
    let (trigger, function) = install_owned_failure(&pool, db_ready.id).await;
    let failed_response = db_client
        .exchange(
            "tools/call",
            public_call("apply_setup", apply_args(db_ready)),
        )
        .await;
    remove_owned_failure(&pool, &trigger, &function).await;
    assert_eq!(failed_response["result"]["isError"], true);
    let failed = tool_payload(&failed_response);
    assert_eq!(failed["error"]["code"], "storage_unavailable");
    assert_apply_retry(&failed, db_ready);
    assert_eq!(
        fs::read_to_string(db_directory.join("AGENTS.md")).unwrap(),
        db_content
    );
    let published_inode = fs::metadata(db_directory.join("AGENTS.md")).unwrap().ino();
    let rolled_back = row(&pool, host.tenant_id, db_ready.id).await;
    assert_eq!(rolled_back.status, "draft");
    assert_eq!(rolled_back.revision, db_ready.revision);
    assert_eq!(rolled_back.applied_from_revision, None);
    let recovered = db_client.call("apply_setup", apply_args(db_ready)).await;
    assert_eq!(
        recovered["file"]["publication"]["outcome"],
        "already_matches"
    );
    assert_eq!(
        fs::metadata(db_directory.join("AGENTS.md")).unwrap().ino(),
        published_inode
    );
    assert_eq!(
        fs::read_to_string(db_directory.join("AGENTS.md")).unwrap(),
        db_content
    );
    assert_single_applied(&pool, host.tenant_id, db_ready, db_content).await;
    db_client.finish().await;

    // Kill the stdio bridge while its real daemon request waits on only this setup row.
    let lost_directory = root.join("lost-response");
    fs::create_dir(&lost_directory).unwrap();
    let lost_content = "# Lost response\n\nRetry the same ready revision.\n";
    let lost_native = Uuid::new_v4().to_string();
    let mut lost_client = Mcp::start(&socket, &config, &lost_native, &workspace_key).await;
    let lost_ready = ready_setup(&mut lost_client, &lost_directory, lost_content).await;
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM workspace_setups WHERE tenant_id=$1 AND id=$2 FOR UPDATE")
        .bind(host.tenant_id)
        .bind(lost_ready.id)
        .execute(&mut *blocker)
        .await
        .unwrap();
    lost_client
        .send(
            "tools/call",
            public_call("apply_setup", apply_args(lost_ready)),
        )
        .await;
    wait_until_blocked(&pool, &tag).await;
    lost_client.kill().await;
    blocker.rollback().await.unwrap();
    let uncertain = wait_until_applied(&pool, host.tenant_id, lost_ready.id).await;
    assert_eq!(uncertain.applied_from_revision, Some(lost_ready.revision));
    let lost_inode = fs::metadata(lost_directory.join("AGENTS.md"))
        .unwrap()
        .ino();
    let mut retry = Mcp::start(&socket, &config, &lost_native, &workspace_key).await;
    let retried = retry.call("apply_setup", apply_args(lost_ready)).await;
    assert_eq!(retried["file"]["publication"]["outcome"], "already_matches");
    assert_eq!(
        fs::metadata(lost_directory.join("AGENTS.md"))
            .unwrap()
            .ino(),
        lost_inode
    );
    assert_single_applied(&pool, host.tenant_id, lost_ready, lost_content).await;

    // Historical applied status cannot restore or overwrite current filesystem state.
    fs::remove_file(lost_directory.join("AGENTS.md")).unwrap();
    let missing = retry
        .call_error("apply_setup", apply_args(lost_ready))
        .await;
    assert_eq!(missing["error"]["code"], "setup_file_conflict");
    assert_reload(&missing, lost_ready);
    assert!(!lost_directory.join("AGENTS.md").exists());
    fs::write(lost_directory.join("AGENTS.md"), lost_content).unwrap();
    fs::write(
        lost_directory.join("AGENTS.md"),
        "foreign changed instructions",
    )
    .unwrap();
    let changed = retry
        .call_error("apply_setup", apply_args(lost_ready))
        .await;
    assert_eq!(changed["error"]["code"], "setup_file_conflict");
    assert_reload(&changed, lost_ready);
    assert_eq!(
        fs::read_to_string(lost_directory.join("AGENTS.md")).unwrap(),
        "foreign changed instructions"
    );
    assert_single_applied(&pool, host.tenant_id, lost_ready, lost_content).await;
    retry.finish().await;

    // Two native sessions share one host and directory; the row lock serializes apply.
    let concurrent_directory = root.join("concurrent");
    fs::create_dir(&concurrent_directory).unwrap();
    let concurrent_content = "# Concurrent setup\n\nPublish once and adopt once.\n";
    let mut first = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    let mut second = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    open_and_bind(&mut first, &concurrent_directory).await;
    open_and_bind(&mut second, &concurrent_directory).await;
    let begun = first
        .call(
            "begin_setup",
            json!({"request_id":Uuid::new_v4(),"input":"Create concurrent recovery fixture instructions."}),
        )
        .await;
    let concurrent_id: Uuid = begun["setup"]["id"].as_str().unwrap().parse().unwrap();
    let saved = first
        .call(
            "save_setup",
            json!({"setup_id":concurrent_id,
                "revision":begun["setup"]["revision"],"input_cursor":1,
                "ready":true,"content":concurrent_content}),
        )
        .await;
    let concurrent_ready = ReadySetup {
        id: concurrent_id,
        revision: saved["setup"]["revision"].as_i64().unwrap(),
    };
    let visible = second
        .call(
            "get_setup",
            json!({"setup_id":concurrent_id,"after_input":0,"limit":25}),
        )
        .await;
    assert_eq!(visible["setup"]["revision"], concurrent_ready.revision);
    let (left, right) = tokio::join!(
        first.call("apply_setup", apply_args(concurrent_ready)),
        second.call("apply_setup", apply_args(concurrent_ready))
    );
    let mut outcomes = vec![
        left["file"]["publication"]["outcome"].as_str().unwrap(),
        right["file"]["publication"]["outcome"].as_str().unwrap(),
    ];
    outcomes.sort_unstable();
    assert_eq!(outcomes, ["already_matches", "created"]);
    assert_eq!(left["setup"]["revision"], concurrent_ready.revision + 1);
    assert_eq!(right["setup"]["revision"], concurrent_ready.revision + 1);
    assert_single_applied(&pool, host.tenant_id, concurrent_ready, concurrent_content).await;

    // A repeat authenticates again; revoking this owned host denies it without recovery calls.
    admin::revoke_host(&pool, host.auth.host_id).await.unwrap();
    let denied = first
        .call_error("apply_setup", apply_args(concurrent_ready))
        .await;
    assert_eq!(denied["error"]["code"], "unauthorized");
    assert_eq!(denied["actions"], json!([]));
    assert_eq!(denied["recommended_action"], Value::Null);
    assert_eq!(
        fs::read_to_string(concurrent_directory.join("AGENTS.md")).unwrap(),
        concurrent_content
    );
    first.finish().await;
    second.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
