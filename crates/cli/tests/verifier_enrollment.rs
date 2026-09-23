use sqlx::PgPool;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use tect_application::WorkspaceService;
use tect_domain::{Error, PrincipalRole, RequestContext};
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

fn context(auth: &tect_domain::HostAuth, key: &str) -> RequestContext {
    RequestContext {
        auth: auth.clone(),
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: key.into(),
    }
}

async fn verifier_counts(pool: &PgPool, tenant_id: Uuid) -> (i64, i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM principals WHERE tenant_id=$1 AND role='verifier'), \
                (SELECT count(*) FROM hosts h JOIN principals p ON p.id=h.principal_id \
                 WHERE h.tenant_id=$1 AND p.role='verifier'), \
                (SELECT count(*) FROM memberships m JOIN principals p ON p.id=m.principal_id \
                 WHERE m.tenant_id=$1 AND p.role='verifier')",
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn verifier_enrollment_is_distinct_pregranted_and_cannot_open_owner_routes() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let store = Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap());
    let service = WorkspaceService::new(
        store,
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    );
    let owner = admin::enroll_host(&pool, None, vec![]).await.unwrap();
    let owner_context = context(&owner.auth, "verifier-existing");
    let workspace = service.open_workspace(&owner_context).await.unwrap();
    let workspace_id = workspace.workspace.unwrap().id;
    let other_workspace = service
        .open_workspace(&context(&owner.auth, "verifier-not-granted"))
        .await
        .unwrap()
        .workspace
        .unwrap()
        .id;

    let before: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM workspaces WHERE tenant_id=$1), \
                (SELECT count(*) FROM memberships WHERE tenant_id=$1)",
    )
    .bind(owner.tenant_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let verifier = admin::prepare_verifier_enrollment(&pool, owner.tenant_id, workspace_id)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    assert_eq!(
        admin::verifier_enrollment_state(&pool, &verifier, workspace_id)
            .await
            .unwrap(),
        admin::VerifierEnrollmentState::Committed
    );
    assert_ne!(verifier.principal_id, owner.principal_id);
    assert_ne!(verifier.auth.host_id, owner.auth.host_id);
    assert_ne!(verifier.auth.credential, owner.auth.credential);
    let actual_role: String = sqlx::query_scalar("SELECT role FROM principals WHERE id=$1")
        .bind(verifier.principal_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(actual_role, "verifier");
    let after: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM workspaces WHERE tenant_id=$1), \
                (SELECT count(*) FROM memberships WHERE tenant_id=$1)",
    )
    .bind(owner.tenant_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(after, (before.0, before.1 + 1));
    let membership: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM memberships WHERE tenant_id=$1 AND workspace_id=$2 AND principal_id=$3)",
    )
    .bind(owner.tenant_id)
    .bind(workspace_id)
    .bind(verifier.principal_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(membership);
    let other_membership: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM memberships WHERE tenant_id=$1 AND workspace_id=$2 AND principal_id=$3)",
    )
    .bind(owner.tenant_id)
    .bind(other_workspace)
    .bind(verifier.principal_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!other_membership);
    let verifier_context = context(&verifier.auth, "verifier-existing");
    let verifier_open = service.open_workspace(&verifier_context).await.unwrap();
    assert_eq!(verifier_open.workspace.as_ref().unwrap().id, workspace_id);
    assert_eq!(
        verifier_open.session.as_ref().unwrap().host_id,
        verifier.auth.host_id
    );
    assert!(verifier_open.programs.is_empty());
    assert!(verifier_open.candidate_sets.is_empty());
    assert!(verifier_open.native_planning.is_empty());
    assert!(verifier_open.next_action.is_none());
    assert_eq!(
        service.open_workspace(&verifier_context).await.unwrap(),
        verifier_open
    );
    assert_eq!(
        service.get_state(&verifier_context).await,
        Err(Error::Forbidden)
    );
    assert_eq!(
        service
            .open_workspace(&context(&verifier.auth, "verifier-not-granted"))
            .await,
        Err(Error::Forbidden)
    );
    assert_eq!(
        service
            .open_workspace(&context(&verifier.auth, "new-workspace"))
            .await,
        Err(Error::Forbidden)
    );
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_sessions WHERE host_id=$1")
        .bind(verifier.auth.host_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sessions, 1);

    let private = tempfile::tempdir().unwrap();
    fs::set_permissions(private.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let auth_path = private.path().canonicalize().unwrap().join("verifier.json");
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_tect-admin"))
        .args([
            "enroll-verifier",
            "--tenant",
            &owner.tenant_id.to_string(),
            "--workspace",
            &workspace_id.to_string(),
            "--out",
            auth_path.to_str().unwrap(),
        ])
        .env("TECT_ADMIN_DATABASE_URL", &admin_url)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::metadata(&auth_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let cli_auth: tect_domain::HostAuth =
        serde_json::from_slice(&fs::read(&auth_path).unwrap()).unwrap();
    assert_ne!(cli_auth.host_id, owner.auth.host_id);
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&cli_auth.credential));
    assert_eq!(
        service
            .open_workspace(&context(&cli_auth, "verifier-existing"))
            .await
            .unwrap()
            .workspace
            .unwrap()
            .id,
        workspace_id
    );

    // Exercise the same public MCP route parser and daemon authorization as a
    // real verifier host. The snapshot covers all current advisory effects.
    let socket = private.path().canonicalize().unwrap().join("verifier.sock");
    let daemon = Daemon::start(
        &tagged_url(&runtime_url, &format!("verifier-{}", Uuid::new_v4())),
        socket.clone(),
    )
    .await;
    let owner_path = private.path().canonicalize().unwrap().join("owner.json");
    recovery_support::host_file(&owner_path, &owner.auth);
    let mut owner_mcp = Mcp::start(
        &socket,
        &owner_path,
        &Uuid::new_v4().to_string(),
        "verifier-existing",
    )
    .await;
    owner_mcp.call("open_workspace", json!({})).await;
    let owner_validation = owner_mcp
        .call_error("command", json!({"route":"program.save","params":{}}))
        .await;
    assert_eq!(owner_validation["error"]["code"], "invalid_arguments");
    assert!(owner_validation["error"]["route_contract"].is_object());
    assert!(!owner_validation["actions"].as_array().unwrap().is_empty());
    owner_mcp.finish().await;
    let mut mcp = Mcp::start(
        &socket,
        &auth_path,
        &Uuid::new_v4().to_string(),
        "verifier-existing",
    )
    .await;
    let public_open = mcp.call("open_workspace", json!({})).await;
    assert_eq!(public_open["workspace"]["id"], json!(workspace_id));
    assert_eq!(public_open["next_action"], json!(null));
    assert_eq!(public_open["actions"], json!([]));
    for key in ["verifier-not-granted", "verifier-never-created"] {
        let mut wrong = Mcp::start(&socket, &auth_path, &Uuid::new_v4().to_string(), key).await;
        assert_eq!(
            wrong.call_error("open_workspace", json!({})).await["error"]["code"],
            "forbidden"
        );
        wrong.finish().await;
    }
    let protected = [
        "workspaces",
        "memberships",
        "agent_sessions",
        "programs",
        "advisory_workspace_config",
        "advisory_workspace_config_history",
        "advisory_opportunity",
        "advisory_dispatch",
        "advisory_scope_source_snapshot",
        "advisory_scope_manifest",
        "advisory_scope_advice",
        "advisory_scope_disposition",
        "advisory_scope_preservation_receipt",
        "advisory_scope_caller_link",
        "advisory_scope_verifier_receipt",
        "advisory_scope_selected_save_observation",
    ];
    let mut baseline = Vec::new();
    for table in protected {
        let count: i64 =
            sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE tenant_id=$1"))
                .bind(owner.tenant_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        baseline.push(count);
    }
    let example_id = Uuid::new_v4();
    for (tool, route, params) in [
        (
            "command",
            "scope.advisory.request",
            json!({
                "request_id":example_id,"candidate_set_id":example_id,"request_preference":"skip"
            }),
        ),
        (
            "command",
            "scope.advisory.disposition",
            json!({
                "opportunity_id":example_id,"candidate_set_id":example_id,
                "request_id":example_id,"advice_id":"a".repeat(64),
                "expected_revision":0,"action":"reject_all",
                "items":[{"alternative_id":"b".repeat(64),"state":"not_selected"}],
                "rationale":"Verifier cannot decide"
            }),
        ),
        (
            "command",
            "program.save",
            json!({
                "program_id":example_id,"revision":1,"input_cursor":1,"complete":false
            }),
        ),
        (
            "command",
            "workspace.advisory.configure",
            json!({
                "expected_revision":0,"mode":"optional",
                "provider_profile_ref":{"id":"test"},
                "model_configuration":{"model":"test"}
            }),
        ),
        ("query", "workspace.advisory.config", json!({})),
        ("command", "future.unknown.route", json!({})),
    ] {
        let denied = mcp
            .call_error(tool, json!({"route":route,"params":params}))
            .await;
        assert_eq!(denied["error"]["code"], "forbidden", "{route}: {denied}");
        assert_eq!(denied["actions"], json!([]));
        assert!(denied["error"].get("route_contract").is_none());
    }
    assert_eq!(
        mcp.call_error("get_state", json!({})).await["error"]["code"],
        "forbidden"
    );
    for (tool, route, params) in [
        ("command", "program.save", json!({})),
        ("command", "workspace.open", json!({"role":"owner"})),
        ("query", "program.save", json!({})),
        ("execute", "scope.advisory.request", json!({})),
    ] {
        let denied = mcp
            .call_error(tool, json!({"route":route,"params":params}))
            .await;
        assert_eq!(
            denied["error"]["code"], "forbidden",
            "{tool}/{route}: {denied}"
        );
        assert_eq!(denied["actions"], json!([]));
        assert_eq!(denied["recommended_action"], json!(null));
        assert!(denied["error"].get("route_contract").is_none());
        assert!(denied["error"].get("tool").is_none());
        assert!(denied["error"].get("route").is_none());
    }
    for (table, before) in protected.into_iter().zip(baseline) {
        let after: i64 =
            sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE tenant_id=$1"))
                .bind(owner.tenant_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(after, before, "{table} changed under verifier request");
    }
    mcp.finish().await;
    drop(daemon);

    let counts_before = verifier_counts(&pool, owner.tenant_id).await;
    let original = fs::read(&auth_path).unwrap();
    let collision = tokio::process::Command::new(env!("CARGO_BIN_EXE_tect-admin"))
        .args([
            "enroll-verifier",
            "--tenant",
            &owner.tenant_id.to_string(),
            "--workspace",
            &workspace_id.to_string(),
            "--out",
            auth_path.to_str().unwrap(),
        ])
        .env("TECT_ADMIN_DATABASE_URL", &admin_url)
        .output()
        .await
        .unwrap();
    assert!(!collision.status.success());
    assert_eq!(fs::read(&auth_path).unwrap(), original);
    assert_eq!(verifier_counts(&pool, owner.tenant_id).await, counts_before);
    let pending = admin::prepare_verifier_enrollment(&pool, owner.tenant_id, workspace_id)
        .await
        .unwrap();
    assert_eq!(verifier_counts(&pool, owner.tenant_id).await, counts_before);
    drop(pending);
    assert_eq!(verifier_counts(&pool, owner.tenant_id).await, counts_before);
    let absent = admin::Enrollment {
        auth: tect_domain::HostAuth {
            host_id: Uuid::new_v4(),
            credential: "0".repeat(64),
        },
        tenant_id: owner.tenant_id,
        principal_id: Uuid::new_v4(),
    };
    assert_eq!(
        admin::verifier_enrollment_state(&pool, &absent, workspace_id)
            .await
            .unwrap(),
        admin::VerifierEnrollmentState::Absent
    );

    let missing = admin::prepare_verifier_enrollment(&pool, owner.tenant_id, Uuid::new_v4()).await;
    assert!(matches!(missing, Err(Error::NotFound)));
    let foreign_owner = admin::enroll_host(&pool, None, vec![]).await.unwrap();
    let foreign =
        admin::prepare_verifier_enrollment(&pool, foreign_owner.tenant_id, workspace_id).await;
    assert!(matches!(foreign, Err(Error::NotFound)));
    let owner_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM principals WHERE tenant_id=$1 AND role='owner'")
            .bind(owner.tenant_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(owner_count, 1);
    let second_owner =
        sqlx::query("INSERT INTO principals (id, tenant_id, role) VALUES ($1, $2, 'owner')")
            .bind(Uuid::new_v4())
            .bind(owner.tenant_id)
            .execute(&pool)
            .await;
    assert!(second_owner.is_err());
    assert_eq!(
        service
            .open_workspace(&context(&owner.auth, "owner-still-works"))
            .await
            .unwrap()
            .workspace
            .unwrap()
            .key,
        "owner-still-works"
    );

    let mut transaction = tect_application::Store::begin(
        &PgStore::connect(&runtime_url, 2).await.unwrap(),
        tect_application::TransactionMode::ReadOnly,
    )
    .await
    .unwrap();
    let identity = transaction.authenticate(&verifier.auth).await.unwrap();
    assert_eq!(identity.role, PrincipalRole::Verifier);
}
mod recovery_support;

use recovery_support::{Daemon, Mcp, tagged_url};
use serde_json::json;
