//! Every Program tool crosses the native host/session/member boundary.
mod recovery_support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::path::Path;
use tect_postgres::admin::{self, Enrollment};
use uuid::Uuid;

struct OpenProgram {
    enrollment: Enrollment,
    client: Mcp,
    program_id: Uuid,
    workspace_id: Uuid,
    session_id: Uuid,
    canary: String,
}

async fn open_program(
    pool: &PgPool,
    socket: &Path,
    root: &Path,
    label: &str,
    tenant: Option<Uuid>,
) -> OpenProgram {
    let enrollment = admin::enroll_host(pool, tenant, Vec::new()).await.unwrap();
    let config = root.join(format!("{label}.json"));
    host_file(&config, &enrollment.auth);
    let workspace = format!("program-access-{label}-{}", Uuid::new_v4().simple());
    let canary = format!("sec01-canary-{label}-{}", Uuid::new_v4().simple());
    let mut client = Mcp::start(socket, &config, &Uuid::new_v4().to_string(), &workspace).await;
    let opened = client.call("open_workspace", json!({})).await;
    let created = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":format!("Program for {label}: {canary}")}),
        )
        .await;
    OpenProgram {
        enrollment,
        workspace_id: Uuid::parse_str(opened["workspace"]["id"].as_str().unwrap()).unwrap(),
        session_id: Uuid::parse_str(opened["session"]["id"].as_str().unwrap()).unwrap(),
        program_id: Uuid::parse_str(created["program"]["id"].as_str().unwrap()).unwrap(),
        canary,
        client,
    }
}

fn calls(program_id: Uuid) -> Vec<(&'static str, Value)> {
    vec![
        (
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"refused creation"}),
        ),
        ("get_program", json!({"program_id":program_id})),
        (
            "save_program",
            json!({"program_id":program_id,"revision":1,"input_cursor":0}),
        ),
        (
            "record_program_input",
            json!({"program_id":program_id,"request_id":Uuid::new_v4(),"input":"refused input"}),
        ),
        ("list_programs", json!({})),
    ]
}

async fn assert_all_denied(client: &mut Mcp, program_id: Uuid, canary: &str, code: &str) {
    for (name, arguments) in calls(program_id) {
        let payload = client.call_error(name, arguments).await;
        assert_eq!(payload["error"]["code"], code, "{name}: {payload}");
        assert!(
            payload["actions"].as_array().unwrap().is_empty(),
            "{name}: {payload}"
        );
        assert!(payload["recommended_action"].is_null(), "{name}: {payload}");
        assert!(!payload.to_string().contains(&program_id.to_string()));
        assert!(!payload.to_string().contains(canary));
        assert!(payload.get("program").is_none());
        assert!(payload.get("programs").is_none());
    }
}

async fn counts(pool: &PgPool, tenant: Uuid) -> (i64, i64) {
    let programs = sqlx::query_scalar("SELECT count(*) FROM programs WHERE tenant_id=$1")
        .bind(tenant)
        .fetch_one(pool)
        .await
        .unwrap();
    let inputs = sqlx::query_scalar("SELECT count(*) FROM program_inputs WHERE tenant_id=$1")
        .bind(tenant)
        .fetch_one(pool)
        .await
        .unwrap();
    (programs, inputs)
}

async fn assert_foreign(client: &mut Mcp, id: Uuid, canary: &str) {
    for (name, arguments) in [
        ("get_program", json!({"program_id":id})),
        (
            "save_program",
            json!({"program_id":id,"revision":1,"input_cursor":0}),
        ),
        (
            "record_program_input",
            json!({"program_id":id,"request_id":Uuid::new_v4(),"input":"foreign"}),
        ),
    ] {
        let payload = client.call_error(name, arguments).await;
        assert_eq!(payload["error"]["code"], "not_found", "{name}: {payload}");
        assert!(!payload.to_string().contains(&id.to_string()));
        assert!(!payload.to_string().contains(canary));
        assert!(payload.get("program").is_none());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn program_tools_hide_foreign_rows_and_honor_every_revocation_layer() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("access.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-program-access-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;

    let mut main = open_program(&pool, &socket, &root, "main", None).await;
    let mut sibling = open_program(
        &pool,
        &socket,
        &root,
        "sibling",
        Some(main.enrollment.tenant_id),
    )
    .await;
    let mut outsider = open_program(&pool, &socket, &root, "outsider", None).await;
    let main_before = counts(&pool, main.enrollment.tenant_id).await;
    let outsider_before = counts(&pool, outsider.enrollment.tenant_id).await;
    assert_foreign(&mut main.client, sibling.program_id, &sibling.canary).await;
    assert_foreign(&mut sibling.client, main.program_id, &main.canary).await;
    assert_foreign(&mut main.client, outsider.program_id, &outsider.canary).await;
    assert_foreign(&mut outsider.client, main.program_id, &main.canary).await;
    assert_eq!(counts(&pool, main.enrollment.tenant_id).await, main_before);
    assert_eq!(
        counts(&pool, outsider.enrollment.tenant_id).await,
        outsider_before
    );
    let sibling_list = sibling.client.call("list_programs", json!({})).await;
    let sibling_ids: Vec<_> = sibling_list["programs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|program| program["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(sibling_ids, [sibling.program_id.to_string()]);
    assert!(!sibling_list.to_string().contains(&main.canary));
    assert!(!sibling_list.to_string().contains(&outsider.canary));

    let mut host_revoked = open_program(&pool, &socket, &root, "host", None).await;
    let host_before = counts(&pool, host_revoked.enrollment.tenant_id).await;
    admin::revoke_host(&pool, host_revoked.enrollment.auth.host_id)
        .await
        .unwrap();
    let denied_help = host_revoked
        .client
        .call_error("read_skill", json!({"name":"tectd-program"}))
        .await;
    assert_eq!(denied_help["error"]["code"], "unauthorized");
    assert_all_denied(
        &mut host_revoked.client,
        host_revoked.program_id,
        &host_revoked.canary,
        "unauthorized",
    )
    .await;
    assert_eq!(
        counts(&pool, host_revoked.enrollment.tenant_id).await,
        host_before
    );

    let mut session_revoked = open_program(&pool, &socket, &root, "session", None).await;
    let session_before = counts(&pool, session_revoked.enrollment.tenant_id).await;
    admin::revoke_session(&pool, session_revoked.session_id)
        .await
        .unwrap();
    let session_help = session_revoked
        .client
        .call("read_skill", json!({"name":"tectd-program"}))
        .await;
    assert_eq!(session_help["method"], "tectd-program");
    assert_all_denied(
        &mut session_revoked.client,
        session_revoked.program_id,
        &session_revoked.canary,
        "session_revoked",
    )
    .await;
    assert_eq!(
        counts(&pool, session_revoked.enrollment.tenant_id).await,
        session_before
    );

    let mut member_revoked = open_program(&pool, &socket, &root, "member", None).await;
    let member_before = counts(&pool, member_revoked.enrollment.tenant_id).await;
    sqlx::query(
        "DELETE FROM memberships WHERE tenant_id=$1 AND workspace_id=$2 AND principal_id=$3",
    )
    .bind(member_revoked.enrollment.tenant_id)
    .bind(member_revoked.workspace_id)
    .bind(member_revoked.enrollment.principal_id)
    .execute(&pool)
    .await
    .unwrap();
    let member_help = member_revoked
        .client
        .call("read_skill", json!({"name":"tectd-program"}))
        .await;
    assert_eq!(member_help["method"], "tectd-program");
    assert_all_denied(
        &mut member_revoked.client,
        member_revoked.program_id,
        &member_revoked.canary,
        "forbidden",
    )
    .await;
    assert_eq!(
        counts(&pool, member_revoked.enrollment.tenant_id).await,
        member_before
    );

    main.client.finish().await;
    sibling.client.finish().await;
    outsider.client.finish().await;
    host_revoked.client.finish().await;
    session_revoked.client.finish().await;
    member_revoked.client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
