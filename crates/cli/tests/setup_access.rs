//! Real PostgreSQL, daemon, and stdio-MCP setup authorization acceptance.

mod recovery_support;
mod setup_support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::json;
use setup_support::{DeniedTarget, assert_denied, setup_table_counts};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use tect_postgres::admin::{self, Enrollment};
use uuid::Uuid;

struct ReadyFixture {
    enrollment: Enrollment,
    client: Mcp,
    setup_id: Uuid,
    revision: i64,
    workspace_id: Uuid,
    session_id: Uuid,
    directory: PathBuf,
    content: String,
    notes: String,
}

async fn start_client(socket: &Path, config: &Path, native: &str, workspace: &str) -> Mcp {
    Mcp::start(socket, config, native, workspace).await
}

async fn ready_fixture(
    pool: &PgPool,
    socket: &Path,
    root: &Path,
    label: &str,
    tenant: Option<Uuid>,
    workspace: &str,
) -> ReadyFixture {
    let directory = root.join(format!("{label}-task"));
    std::fs::create_dir(&directory).unwrap();
    let enrollment = admin::enroll_host_with_grants(
        pool,
        tenant,
        Vec::new(),
        vec![root.to_str().unwrap().to_owned()],
    )
    .await
    .unwrap();
    let config = root.join(format!("{label}-host.json"));
    host_file(&config, &enrollment.auth);
    let mut client = start_client(socket, &config, &Uuid::new_v4().to_string(), workspace).await;
    let opened = client.call("open_workspace", json!({})).await;
    client
        .call(
            "inspect_setup",
            json!({"task_directory":directory.to_str().unwrap()}),
        )
        .await;
    let narrative = format!("protected original narrative for {label}");
    let begun = client
        .call(
            "begin_setup",
            json!({"request_id":Uuid::new_v4(),"input":narrative}),
        )
        .await;
    let setup_id = begun["setup"]["id"].as_str().unwrap().parse().unwrap();
    let content = format!("# Protected {label}\n\nprivate fixture body\n");
    let notes = format!("protected working notes for {label}");
    let saved = client
        .call(
            "save_setup",
            json!({
                "setup_id":setup_id,"revision":1,"input_cursor":1,"ready":true,
                "content":content,"working_notes":notes
            }),
        )
        .await;
    ReadyFixture {
        enrollment,
        client,
        setup_id,
        revision: saved["setup"]["revision"].as_i64().unwrap(),
        workspace_id: opened["workspace"]["id"].as_str().unwrap().parse().unwrap(),
        session_id: opened["session"]["id"].as_str().unwrap().parse().unwrap(),
        directory,
        content,
        notes,
    }
}

impl ReadyFixture {
    fn target(&self) -> DeniedTarget {
        DeniedTarget {
            setup_id: self.setup_id,
            revision: self.revision,
            directory: self.directory.clone(),
            content: self.content.clone(),
            notes: self.notes.clone(),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn setup_tools_fail_closed_across_every_identity_and_directory_boundary() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let denied_temp = private_temp();
    let denied_directory = denied_temp
        .path()
        .canonicalize()
        .unwrap()
        .join("denied-task");
    std::fs::create_dir(&denied_directory).unwrap();
    let socket = root.join("access.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-setup-access-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let workspace = format!("setup-access-{}", Uuid::new_v4().simple());

    let enrollment = admin::enroll_host_with_grants(
        &pool,
        None,
        Vec::new(),
        vec![root.to_str().unwrap().to_owned()],
    )
    .await
    .unwrap();
    let config = root.join("status-host.json");
    host_file(&config, &enrollment.auth);

    let mut unbound = start_client(&socket, &config, &Uuid::new_v4().to_string(), &workspace).await;
    unbound.call("open_workspace", json!({})).await;
    let before_unbound = setup_table_counts(&pool, enrollment.tenant_id).await;
    let no_directory = unbound
        .call_error(
            "begin_setup",
            json!({"request_id":Uuid::new_v4(),"input":"must remain unbound"}),
        )
        .await;
    assert_eq!(no_directory["error"]["code"], "task_directory_unbound");
    assert_eq!(
        setup_table_counts(&pool, enrollment.tenant_id).await,
        before_unbound
    );
    let unknown = unbound.call("inspect_setup", json!({})).await;
    assert_eq!(unknown["file"]["status"], "context_unknown");
    assert_eq!(unknown["file"]["observed_now"], false);
    assert_eq!(
        setup_table_counts(&pool, enrollment.tenant_id).await,
        before_unbound
    );

    let existing_directory = root.join("existing-task");
    std::fs::create_dir(&existing_directory).unwrap();
    let existing_body = "foreign existing instructions\n";
    std::fs::write(existing_directory.join("AGENTS.md"), existing_body).unwrap();
    let mut existing =
        start_client(&socket, &config, &Uuid::new_v4().to_string(), &workspace).await;
    existing.call("open_workspace", json!({})).await;
    let observed = existing
        .call(
            "inspect_setup",
            json!({"task_directory":existing_directory.to_str().unwrap()}),
        )
        .await;
    assert_eq!(observed["file"]["status"], "existing");
    assert!(!observed.to_string().contains(existing_body));
    let before_existing = setup_table_counts(&pool, enrollment.tenant_id).await;
    let conflict = existing
        .call_error(
            "begin_setup",
            json!({"request_id":Uuid::new_v4(),"input":"do not overwrite"}),
        )
        .await;
    assert_eq!(conflict["error"]["code"], "setup_file_conflict");
    assert_eq!(
        setup_table_counts(&pool, enrollment.tenant_id).await,
        before_existing
    );
    assert_eq!(
        std::fs::read_to_string(existing_directory.join("AGENTS.md")).unwrap(),
        existing_body
    );

    let mut denied = start_client(&socket, &config, &Uuid::new_v4().to_string(), &workspace).await;
    denied.call("open_workspace", json!({})).await;
    let before_denied = setup_table_counts(&pool, enrollment.tenant_id).await;
    let unavailable = denied
        .call(
            "inspect_setup",
            json!({"task_directory":denied_directory.to_str().unwrap()}),
        )
        .await;
    assert_eq!(unavailable["file"]["status"], "unavailable");
    assert_eq!(unavailable["file"]["reason"], "setup_root_not_granted");
    assert_eq!(
        setup_table_counts(&pool, enrollment.tenant_id).await,
        before_denied
    );
    let still_unbound = denied
        .call_error(
            "begin_setup",
            json!({"request_id":Uuid::new_v4(),"input":"denied narrative"}),
        )
        .await;
    assert_eq!(still_unbound["error"]["code"], "task_directory_unbound");
    assert_eq!(
        setup_table_counts(&pool, enrollment.tenant_id).await,
        before_denied
    );

    let mut main = ready_fixture(
        &pool,
        &socket,
        &root,
        "main",
        Some(enrollment.tenant_id),
        &workspace,
    )
    .await;
    assert_eq!(main.enrollment.principal_id, enrollment.principal_id);
    assert!(!main.directory.join("AGENTS.md").exists());
    let main_target = main.target();

    let foreign_host = admin::enroll_host_with_grants(
        &pool,
        Some(main.enrollment.tenant_id),
        Vec::new(),
        vec![root.to_str().unwrap().to_owned()],
    )
    .await
    .unwrap();
    let foreign_host_config = root.join("foreign-host.json");
    host_file(&foreign_host_config, &foreign_host.auth);
    let mut cross_host = start_client(
        &socket,
        &foreign_host_config,
        &Uuid::new_v4().to_string(),
        &workspace,
    )
    .await;
    cross_host.call("open_workspace", json!({})).await;
    cross_host
        .call(
            "inspect_setup",
            json!({"task_directory":main.directory.to_str().unwrap()}),
        )
        .await;
    assert_denied(&pool, &mut cross_host, &main_target, "not_found").await;

    let other_tenant = admin::enroll_host_with_grants(
        &pool,
        None,
        Vec::new(),
        vec![root.to_str().unwrap().to_owned()],
    )
    .await
    .unwrap();
    let other_tenant_config = root.join("other-tenant.json");
    host_file(&other_tenant_config, &other_tenant.auth);
    let mut cross_tenant = start_client(
        &socket,
        &other_tenant_config,
        &Uuid::new_v4().to_string(),
        &workspace,
    )
    .await;
    cross_tenant.call("open_workspace", json!({})).await;
    cross_tenant
        .call(
            "inspect_setup",
            json!({"task_directory":main.directory.to_str().unwrap()}),
        )
        .await;
    assert_denied(&pool, &mut cross_tenant, &main_target, "not_found").await;

    let main_native =
        main.client.call("get_state", json!({})).await["session"]["native_session_id"]
            .as_str()
            .unwrap()
            .to_owned();
    let mut wrong_key = start_client(
        &socket,
        &root.join("main-host.json"),
        &main_native,
        "wrong-key",
    )
    .await;
    assert_denied(
        &pool,
        &mut wrong_key,
        &main_target,
        "session_workspace_mismatch",
    )
    .await;

    let mut unknown_native = start_client(
        &socket,
        &root.join("main-host.json"),
        &Uuid::new_v4().to_string(),
        &workspace,
    )
    .await;
    unknown_native.call("open_workspace", json!({})).await;
    assert_denied(
        &pool,
        &mut unknown_native,
        &main_target,
        "task_directory_unbound",
    )
    .await;

    let other_directory = root.join("bound-elsewhere");
    std::fs::create_dir(&other_directory).unwrap();
    let mut elsewhere = start_client(
        &socket,
        &root.join("main-host.json"),
        &Uuid::new_v4().to_string(),
        &workspace,
    )
    .await;
    elsewhere.call("open_workspace", json!({})).await;
    elsewhere
        .call(
            "inspect_setup",
            json!({"task_directory":other_directory.to_str().unwrap()}),
        )
        .await;
    assert_denied(&pool, &mut elsewhere, &main_target, "not_found").await;

    sqlx::query("UPDATE hosts SET allowed_setup_roots='[]'::jsonb WHERE id=$1")
        .bind(main.enrollment.auth.host_id)
        .execute(&pool)
        .await
        .unwrap();
    let db_only = main.client.call("get_state", json!({})).await;
    assert_eq!(
        db_only["setup_context"]["setup"]["id"],
        main.setup_id.to_string()
    );
    assert_eq!(db_only["file"]["status"], "context_unknown");
    assert_eq!(db_only["file"]["observed_now"], false);
    assert_denied(&pool, &mut main.client, &main_target, "setup_unavailable").await;
    sqlx::query("UPDATE hosts SET allowed_setup_roots=$2 WHERE id=$1")
        .bind(main.enrollment.auth.host_id)
        .bind(sqlx::types::Json(vec![root.to_str().unwrap().to_owned()]))
        .execute(&pool)
        .await
        .unwrap();
    let restored = main
        .client
        .call(
            "get_setup",
            json!({"setup_id":main.setup_id,"after_input":0,"limit":25}),
        )
        .await;
    assert_eq!(restored["setup"]["content"], main.content);

    let mut member = ready_fixture(
        &pool,
        &socket,
        &root,
        "member",
        None,
        &format!("member-{}", Uuid::new_v4()),
    )
    .await;
    sqlx::query(
        "DELETE FROM memberships WHERE tenant_id=$1 AND workspace_id=$2 AND principal_id=$3",
    )
    .bind(member.enrollment.tenant_id)
    .bind(member.workspace_id)
    .bind(member.enrollment.principal_id)
    .execute(&pool)
    .await
    .unwrap();
    let member_target = member.target();
    assert_denied(&pool, &mut member.client, &member_target, "forbidden").await;

    let mut revoked_session = ready_fixture(
        &pool,
        &socket,
        &root,
        "session",
        None,
        &format!("session-{}", Uuid::new_v4()),
    )
    .await;
    admin::revoke_session(&pool, revoked_session.session_id)
        .await
        .unwrap();
    let revoked_session_target = revoked_session.target();
    assert_denied(
        &pool,
        &mut revoked_session.client,
        &revoked_session_target,
        "session_revoked",
    )
    .await;

    let mut revoked_host = ready_fixture(
        &pool,
        &socket,
        &root,
        "host",
        None,
        &format!("host-{}", Uuid::new_v4()),
    )
    .await;
    admin::revoke_host(&pool, revoked_host.enrollment.auth.host_id)
        .await
        .unwrap();
    let revoked_host_target = revoked_host.target();
    assert_denied(
        &pool,
        &mut revoked_host.client,
        &revoked_host_target,
        "unauthorized",
    )
    .await;

    unbound.finish().await;
    existing.finish().await;
    denied.finish().await;
    main.client.finish().await;
    cross_host.finish().await;
    cross_tenant.finish().await;
    wrong_key.finish().await;
    unknown_native.finish().await;
    elsewhere.finish().await;
    member.client.finish().await;
    revoked_session.client.finish().await;
    revoked_host.client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
