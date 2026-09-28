use sqlx::PgPool;
use std::sync::Arc;
use tect_application::WorkspaceService;
use tect_domain::{AdvisoryRequestPreference, Error, RequestContext, SetSessionAdvisoryPreference};
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

async fn service(runtime_url: &str) -> WorkspaceService {
    WorkspaceService::new(
        Arc::new(PgStore::connect(runtime_url, 4).await.unwrap()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    )
}

#[tokio::test]
#[ignore = "requires fresh disposable PostgreSQL 18 and TECT_TEST_* URLs"]
async fn native_session_preference_is_durable_isolated_and_cas_guarded() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    let version: i32 = sqlx::query_scalar("SELECT current_setting('server_version_num')::integer")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!((180000..190000).contains(&version));
    admin::migrate(&pool, &role).await.unwrap();
    let enrollment = admin::enroll_host(&pool, None, vec![]).await.unwrap();
    let key = format!("preference-{}", Uuid::new_v4());
    let workspace = Uuid::new_v4();
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(enrollment.tenant_id)
        .bind(&key)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
        .bind(enrollment.tenant_id)
        .bind(workspace)
        .bind(enrollment.principal_id)
        .execute(&pool)
        .await
        .unwrap();
    let natives = [Uuid::new_v4().to_string(), Uuid::new_v4().to_string()];
    let sessions = [Uuid::new_v4(), Uuid::new_v4()];
    for index in 0..2 {
        sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
            .bind(sessions[index]).bind(enrollment.tenant_id).bind(enrollment.auth.host_id)
            .bind(workspace).bind(&natives[index]).execute(&pool).await.unwrap();
    }
    let contexts: Vec<_> = natives
        .iter()
        .map(|native| RequestContext {
            auth: enrollment.auth.clone(),
            native_session_id: native.clone(),
            workspace_key: key.clone(),
        })
        .collect();
    let first_service = service(&runtime_url).await;
    for context in &contexts {
        let value = first_service
            .session_advisory_preference(context)
            .await
            .unwrap();
        assert_eq!(value.preference, AdvisoryRequestPreference::UseWorkspace);
        assert_eq!(value.revision, 0);
    }
    let set = SetSessionAdvisoryPreference {
        expected_revision: 0,
        preference: AdvisoryRequestPreference::Skip,
    };
    let changed = first_service
        .set_session_advisory_preference(&contexts[0], &set)
        .await
        .unwrap();
    assert_eq!(changed.revision, 1);
    assert_eq!(changed.preference, AdvisoryRequestPreference::Skip);
    assert_eq!(
        first_service
            .set_session_advisory_preference(&contexts[0], &set)
            .await,
        Err(Error::StaleRevision)
    );
    let reloaded = service(&runtime_url).await;
    assert_eq!(
        reloaded
            .session_advisory_preference(&contexts[0])
            .await
            .unwrap(),
        changed
    );
    assert_eq!(
        reloaded
            .session_advisory_preference(&contexts[1])
            .await
            .unwrap()
            .preference,
        AdvisoryRequestPreference::UseWorkspace
    );
    let history: Vec<(i64, String)> = sqlx::query_as(
        "SELECT revision,preference FROM session_advisory_preference_history \
         WHERE tenant_id=$1 AND workspace_id=$2 AND session_id=$3 ORDER BY revision",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace)
    .bind(sessions[0])
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(history, [(0, "use_workspace".into()), (1, "skip".into())]);
    let other_history: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM session_advisory_preference_history WHERE session_id=$1",
    )
    .bind(sessions[1])
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(other_history, 0);

    let mut forged_workspace = contexts[0].clone();
    forged_workspace.workspace_key = "different-workspace".into();
    assert_eq!(
        reloaded
            .session_advisory_preference(&forged_workspace)
            .await,
        Err(Error::SessionWorkspaceMismatch)
    );
    assert_eq!(
        reloaded
            .set_session_advisory_preference(
                &forged_workspace,
                &SetSessionAdvisoryPreference {
                    expected_revision: 1,
                    preference: AdvisoryRequestPreference::UseWorkspace,
                }
            )
            .await,
        Err(Error::SessionWorkspaceMismatch)
    );
    let mut unbound = contexts[0].clone();
    unbound.native_session_id = Uuid::new_v4().to_string();
    assert_eq!(
        reloaded.session_advisory_preference(&unbound).await,
        Err(Error::WorkspaceNotOpen)
    );
    // Exercise the public bridge through separate native threads with the same
    // host principal and workspace. The body has no target-session selector.
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("session-preference.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-session-pref-{}", Uuid::new_v4()),
    );
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let config_path = root.join("host.json");
    host_file(&config_path, &enrollment.auth);
    let mut client_a = Mcp::start(&socket, &config_path, &natives[0], &key).await;
    let mut client_b = Mcp::start(&socket, &config_path, &natives[1], &key).await;
    let read_a = client_a
        .call(
            "query",
            json!({"route":"session.advisory.preference","params":{}}),
        )
        .await;
    assert_eq!(read_a["preference"], "skip");
    assert_eq!(read_a["revision"], 1);
    let read_b = client_b
        .call(
            "query",
            json!({"route":"session.advisory.preference","params":{}}),
        )
        .await;
    assert_eq!(read_b["preference"], "use_workspace");
    assert_eq!(read_b["revision"], 0);
    let set = client_b.call("command", json!({"route":"session.advisory.preference.set","params":{"expected_revision":0,"preference":"skip"}})).await;
    assert_eq!(set["preference"], "skip");
    assert_eq!(set["revision"], 1);
    let repeated = client_b.call_error("command", json!({"route":"session.advisory.preference.set","params":{"expected_revision":0,"preference":"skip"}})).await;
    assert_eq!(repeated["error"]["code"], "stale_revision");
    let after_a = client_a
        .call(
            "query",
            json!({"route":"session.advisory.preference","params":{}}),
        )
        .await;
    assert_eq!(after_a["preference"], "skip");
    assert_eq!(after_a["revision"], 1);
    let after_b = client_b
        .call(
            "query",
            json!({"route":"session.advisory.preference","params":{}}),
        )
        .await;
    assert_eq!(after_b["preference"], "skip");
    assert_eq!(after_b["revision"], 1);

    let forged_get = client_b
        .call_error(
            "query",
            json!({"route":"session.advisory.preference","params":{"session_id":sessions[0]}}),
        )
        .await;
    assert_eq!(forged_get["error"]["code"], "invalid_arguments");
    let forged_set = client_b.call_error("command", json!({"route":"session.advisory.preference.set","params":{"expected_revision":1,"preference":"use_workspace","workspace_id":workspace}})).await;
    assert_eq!(forged_set["error"]["code"], "invalid_arguments");
    let unchanged_b = client_b
        .call(
            "query",
            json!({"route":"session.advisory.preference","params":{}}),
        )
        .await;
    assert_eq!(unchanged_b["preference"], "skip");
    assert_eq!(unchanged_b["revision"], 1);

    let mut unbound_client =
        Mcp::start(&socket, &config_path, &unbound.native_session_id, &key).await;
    let unbound_get = unbound_client
        .call_error(
            "query",
            json!({"route":"session.advisory.preference","params":{}}),
        )
        .await;
    assert_eq!(unbound_get["error"]["code"], "workspace_not_open");
    let unbound_set = unbound_client.call_error("command", json!({"route":"session.advisory.preference.set","params":{"expected_revision":0,"preference":"skip"}})).await;
    assert_eq!(unbound_set["error"]["code"], "workspace_not_open");

    sqlx::query("UPDATE agent_sessions SET revoked=true WHERE id=$1")
        .bind(sessions[0])
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        reloaded.session_advisory_preference(&contexts[0]).await,
        Err(Error::SessionRevoked)
    );
    assert_eq!(
        reloaded
            .set_session_advisory_preference(
                &contexts[0],
                &SetSessionAdvisoryPreference {
                    expected_revision: 1,
                    preference: AdvisoryRequestPreference::UseWorkspace,
                }
            )
            .await,
        Err(Error::SessionRevoked)
    );
    let revoked_get = client_a
        .call_error(
            "query",
            json!({"route":"session.advisory.preference","params":{}}),
        )
        .await;
    assert_eq!(revoked_get["error"]["code"], "session_revoked");
    let revoked_set = client_a.call_error("command", json!({"route":"session.advisory.preference.set","params":{"expected_revision":1,"preference":"use_workspace"}})).await;
    assert_eq!(revoked_set["error"]["code"], "session_revoked");
}
#[allow(dead_code)]
mod recovery_support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::json;
