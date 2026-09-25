#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
mod support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use support::{id, ready_source_candidate, repository, route, route_error};
use tect_application::WorkspaceService;
use tect_domain::{
    AdvisoryModelConfiguration, AdvisoryProviderProfileRef, ConfigureWorkspaceAdvisory, Error,
    RequestContext, WorkspaceAdvisoryMode,
};
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

fn configure(expected_revision: i64, provider: &str) -> ConfigureWorkspaceAdvisory {
    ConfigureWorkspaceAdvisory {
        expected_revision,
        mode: WorkspaceAdvisoryMode::Optional,
        provider_profile_ref: Some(AdvisoryProviderProfileRef {
            id: provider.into(),
        }),
        model_configuration: Some(AdvisoryModelConfiguration {
            model: "jev-advisory-v1".into(),
        }),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires disposable PostgreSQL 18.6 and TECT_TEST_*; run with `cargo test -p tect-cli --test advisory_slice_zero -- --ignored --nocapture`"]
async fn slice_zero_is_live_tenant_safe_durable_and_routed() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();

    let migration_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success")
            .fetch_one(&pool)
            .await
            .unwrap();
    let expected_migration_count =
        i64::try_from(sqlx::migrate!("../postgres/migrations").iter().count()).unwrap();
    assert_eq!(migration_count, expected_migration_count);
    let server_version: String = sqlx::query_scalar("SHOW server_version_num")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(server_version.parse::<i32>().unwrap() / 10_000, 18);

    let privileges: bool = sqlx::query_scalar(
        "SELECT has_table_privilege($1,'advisory_workspace_config','SELECT,INSERT') \
         AND has_column_privilege($1,'advisory_workspace_config','revision','UPDATE') \
         AND NOT has_table_privilege($1,'advisory_workspace_config_history','UPDATE') \
         AND NOT has_table_privilege($1,'advisory_dispatch','DELETE')",
    )
    .bind(&role)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(privileges);

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("slice-zero.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-slice-zero-{}", Uuid::new_v4()));
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;

    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config_path = root.join("host.json");
    host_file(&config_path, &enrollment.auth);
    let key = format!("slice-zero-{}", Uuid::new_v4());
    let native_a = Uuid::new_v4().to_string();
    let native_b = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config_path, &native_a, &key).await;
    let mut peer = Mcp::start(&socket, &config_path, &native_b, &key).await;
    let opened = client.call("open_workspace", json!({})).await;
    peer.call("open_workspace", json!({})).await;
    let workspace_id = id(&opened["workspace"]["id"]);

    let initial = route(&mut client, "query", "workspace.advisory.config", json!({})).await;
    assert_eq!(initial["revision"], 0);
    assert_eq!(initial["mode"], "disabled");
    assert_eq!(initial["materialized"], false);

    let direct = Arc::new(WorkspaceService::new(
        Arc::new(PgStore::connect(&runtime_url, 8).await.unwrap()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    ));
    let context_a = RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: native_a.clone(),
        workspace_key: key.clone(),
    };
    let context_b = RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: native_b,
        workspace_key: key.clone(),
    };
    let request_a = configure(0, "jev-a");
    let request_b = configure(0, "jev-b");
    let (left, right) = tokio::join!(
        direct.configure_advisory(&context_a, &request_a),
        direct.configure_advisory(&context_b, &request_b)
    );
    assert!(matches!(
        (&left, &right),
        (Ok(config), Err(Error::StaleRevision)) | (Err(Error::StaleRevision), Ok(config))
            if config.revision == 1
    ));

    let configured = route(
        &mut client,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision": 1,
            "mode": "optional",
            "provider_profile_ref": {"id":"jev-route"},
            "model_configuration": {"model":"jev-advisory-v1"}
        }),
    )
    .await;
    assert_eq!(configured["revision"], 2);
    let stale = route_error(
        &mut client,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision": 1,
            "mode": "disabled"
        }),
    )
    .await;
    assert_eq!(stale["error"]["code"], "stale_revision");

    let history: Vec<i64> = sqlx::query_scalar(
        "SELECT revision FROM advisory_workspace_config_history \
         WHERE tenant_id=$1 AND workspace_id=$2 ORDER BY revision",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(history, [0, 1, 2]);

    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;
    let candidate_set_id = id(&source["candidate_set"]["id"]);
    let native_scopes_before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 AND source_candidate_set_id=$3",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(native_scopes_before, 0);
    route(
        &mut client,
        "query",
        "candidate.advisory.audit",
        json!({"candidate_set_id":candidate_set_id,"limit":10}),
    )
    .await;
    let opened_scope = route(
        &mut client,
        "command",
        "scope.open",
        json!({
            "request_id":Uuid::new_v4(),
            "candidate_set_id":source["candidate_set"]["id"],
            "candidate_set_revision":source["candidate_set"]["revision"],
            "candidate_snapshot_id":source["snapshot"]["id"],
            "candidate_id":candidate["id"],
            "candidate_revision":candidate["revision"]
        }),
    )
    .await;
    let scope_id = id(&opened_scope["created"]["planning"]["scope"]["id"]);

    let before_invalid: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let (program_id, program_revision): (Uuid, String) = sqlx::query_as(
        "SELECT work_item_id,source_revision FROM advisory_opportunity \
         WHERE tenant_id=$1 AND workspace_id=$2 AND work_item_kind='program' \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let invalid_request = Uuid::new_v4();
    let invalid = client
        .call_error(
            "begin_candidate_set",
            json!({
                "request_id":invalid_request,
                "program_id":program_id,
                "program_revision":program_revision.parse::<i64>().unwrap(),
                "boundary":"finite",
                "input":"invalid\u{0}input"
            }),
        )
        .await;
    assert_eq!(invalid["error"]["code"], "invalid_arguments");
    let invalid_row: (String, String) = sqlx::query_as(
        "SELECT state,primary_reason FROM advisory_opportunity \
         WHERE tenant_id=$1 AND workspace_id=$2 AND request_key LIKE $3",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(format!("%{invalid_request}%"))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        invalid_row,
        ("no_call".into(), "deterministic_input_invalid".into())
    );
    let after_invalid: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(after_invalid, before_invalid + 1);

    let (session_id, principal_id): (Uuid, Uuid) = sqlx::query_as(
        "SELECT s.id,h.principal_id FROM agent_sessions s JOIN hosts h \
         ON h.tenant_id=s.tenant_id AND h.id=s.host_id \
         WHERE s.tenant_id=$1 AND s.native_session_id=$2",
    )
    .bind(enrollment.tenant_id)
    .bind(&native_a)
    .fetch_one(&pool)
    .await
    .unwrap();
    let opportunity_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,scope_id,work_item_kind,work_item_id,session_id,authorized_actor_id,source_revision,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) \
         VALUES($1,$2,$3,$4,'scope',$4,$5,$6,'1','scope_decomposition','scope.decomposition.before_selection',2,'use_workspace','use_workspace','slice-00.v1',$7,$8,'prepared','dispatch_authorized')",
    )
    .bind(opportunity_id)
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(scope_id)
    .bind(session_id)
    .bind(principal_id)
    .bind(format!("scope-audit-{opportunity_id}"))
    .bind("1".repeat(64))
    .execute(&pool)
    .await
    .unwrap();
    let dispatch_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,state,send_certainty,retry_basis,send_started_at) \
         VALUES($1,$2,$3,$4,1,'jev-route','jev-advisory-v1','{}',$5,$6,$7,'request','sending','sent_unknown','initial',clock_timestamp())",
    )
    .bind(dispatch_id)
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(opportunity_id)
    .bind("2".repeat(64))
    .bind("1".repeat(64))
    .bind("3".repeat(64))
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        sqlx::query(
            "UPDATE advisory_dispatch SET state='cancelled',sealed_at=clock_timestamp() WHERE id=$1"
        )
        .bind(dispatch_id)
        .execute(&pool)
        .await
        .is_err()
    );

    let retry_opportunity = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,scope_id,work_item_kind,work_item_id,session_id,authorized_actor_id,source_revision,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) \
         VALUES($1,$2,$3,$4,'scope',$4,$5,$6,'1','scope_decomposition','scope.decomposition.before_selection',2,'use_workspace','use_workspace','slice-00.v1',$7,$8,'prepared','dispatch_authorized')",
    )
    .bind(retry_opportunity)
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(scope_id)
    .bind(session_id)
    .bind(principal_id)
    .bind(format!("retry-audit-{retry_opportunity}"))
    .bind("4".repeat(64))
    .execute(&pool)
    .await
    .unwrap();
    let predecessor = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,state,send_certainty,retry_basis,sealed_at) \
         VALUES($1,$2,$3,$4,1,'jev-route','jev-advisory-v1','{}',$5,$6,$7,'request','cancelled','not_sent','initial',clock_timestamp())",
    )
    .bind(predecessor)
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(retry_opportunity)
    .bind("5".repeat(64))
    .bind("4".repeat(64))
    .bind("6".repeat(64))
    .execute(&pool)
    .await
    .unwrap();
    let retry_insert = |attempt: i32, id: Uuid| {
        sqlx::query(
            "INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,predecessor_dispatch_id,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,state,send_certainty,retry_basis) \
             VALUES($1,$2,$3,$4,$5,$6,'jev-route','jev-advisory-v1','{}',$7,$8,$9,'retry','authorized','not_sent','proven_not_sent')",
        )
        .bind(id)
        .bind(enrollment.tenant_id)
        .bind(workspace_id)
        .bind(retry_opportunity)
        .bind(attempt)
        .bind(predecessor)
        .bind("5".repeat(64))
        .bind("4".repeat(64))
        .bind("7".repeat(64))
    };
    retry_insert(2, Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        retry_insert(3, Uuid::new_v4())
            .execute(&pool)
            .await
            .is_err()
    );

    let workspace_audit = route(
        &mut client,
        "query",
        "workspace.advisory.audit",
        json!({"limit":100}),
    )
    .await;
    assert!(
        workspace_audit["aggregate"]["send_unknown_attempts"]
            .as_i64()
            .unwrap()
            >= 1
    );
    let scope_audit = route(
        &mut client,
        "query",
        "scope.advisory.audit",
        json!({"scope_id":scope_id,"limit":10}),
    )
    .await;
    assert!(
        scope_audit["opportunities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == opportunity_id.to_string())
    );
    let detail = route(
        &mut client,
        "query",
        "scope.advisory.get",
        json!({"scope_id":scope_id,"opportunity_id":opportunity_id}),
    )
    .await;
    assert_eq!(detail["dispatches"][0]["send_certainty"], "sent_unknown");
    assert_eq!(detail["dispatches"][0]["request_bytes"], 7);

    let candidate_opportunity = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,scope_id,work_item_kind,work_item_id,session_id,authorized_actor_id,source_revision,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) \
         VALUES($1,$2,$3,NULL,'scope_candidate_set',$4,$5,$6,'1','scope_decomposition','scope.decomposition.before_selection',2,'use_workspace','use_workspace','slice-00.v1',$7,$8,'no_call','request_skip')",
    )
    .bind(candidate_opportunity)
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(session_id)
    .bind(principal_id)
    .bind(format!("candidate-audit-{candidate_opportunity}"))
    .bind("8".repeat(64))
    .execute(&pool)
    .await
    .unwrap();
    let second_candidate_opportunity = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,scope_id,work_item_kind,work_item_id,session_id,authorized_actor_id,source_revision,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) \
         SELECT $2,tenant_id,workspace_id,scope_id,work_item_kind,work_item_id,session_id,authorized_actor_id,source_revision,capability,decision_point,config_revision,session_preference,request_preference,policy_version,$3,material_digest,state,primary_reason FROM advisory_opportunity WHERE id=$1",
    )
    .bind(candidate_opportunity)
    .bind(second_candidate_opportunity)
    .bind(format!("candidate-audit-{second_candidate_opportunity}"))
    .execute(&pool)
    .await
    .unwrap();
    let candidate_audit = route(
        &mut client,
        "query",
        "candidate.advisory.audit",
        json!({"candidate_set_id":candidate_set_id,"limit":100}),
    )
    .await;
    assert!(
        candidate_audit["opportunities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == candidate_opportunity.to_string())
    );
    assert!(
        !candidate_audit["opportunities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == opportunity_id.to_string())
    );
    assert!(
        candidate_audit["aggregate"]["no_call_by_reason"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["reason"] == "request_skip" && row["count"].as_i64().unwrap() >= 2)
    );
    let first_page = route(
        &mut client,
        "query",
        "candidate.advisory.audit",
        json!({"candidate_set_id":candidate_set_id,"limit":1,"reason":"request_skip"}),
    )
    .await;
    let second_page = route(&mut client, "query", "candidate.advisory.audit", json!({"candidate_set_id":candidate_set_id,"limit":1,"reason":"request_skip","after":first_page["next_after"]})).await;
    assert_ne!(
        first_page["opportunities"][0]["id"],
        second_page["opportunities"][0]["id"]
    );
    assert_eq!(first_page["aggregate"], second_page["aggregate"]);
    let candidate_detail = route(
        &mut client,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":candidate_set_id,"opportunity_id":candidate_opportunity}),
    )
    .await;
    assert_eq!(
        candidate_detail["opportunity"]["work_item_id"],
        candidate_set_id.to_string()
    );
    assert_eq!(
        route_error(
            &mut client,
            "query",
            "candidate.advisory.get",
            json!({"candidate_set_id":candidate_set_id,"opportunity_id":opportunity_id})
        )
        .await["error"]["code"],
        "not_found"
    );
    assert_eq!(
        route_error(
            &mut client,
            "query",
            "candidate.advisory.audit",
            json!({"candidate_set_id":Uuid::new_v4(),"limit":10})
        )
        .await["error"]["code"],
        "not_found"
    );

    let mut other_workspace = Mcp::start(
        &socket,
        &config_path,
        &Uuid::new_v4().to_string(),
        &format!("candidate-other-workspace-{}", Uuid::new_v4()),
    )
    .await;
    other_workspace.call("open_workspace", json!({})).await;
    assert_eq!(
        route_error(
            &mut other_workspace,
            "query",
            "candidate.advisory.audit",
            json!({"candidate_set_id":candidate_set_id,"limit":10}),
        )
        .await["error"]["code"],
        "not_found"
    );

    let enrollment_b = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let config_b = root.join("host-b.json");
    host_file(&config_b, &enrollment_b.auth);
    let mut tenant_b = Mcp::start(
        &socket,
        &config_b,
        &Uuid::new_v4().to_string(),
        &format!("tenant-b-{}", Uuid::new_v4()),
    )
    .await;
    tenant_b.call("open_workspace", json!({})).await;
    assert_eq!(
        route_error(
            &mut tenant_b,
            "query",
            "candidate.advisory.get",
            json!({"candidate_set_id":candidate_set_id,"opportunity_id":candidate_opportunity})
        )
        .await["error"]["code"],
        "not_found"
    );
    route(
        &mut tenant_b,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":0,"mode":"optional",
            "provider_profile_ref":{"id":"jev-b"},
            "model_configuration":{"model":"jev-advisory-v1"}
        }),
    )
    .await;
    let empty_b = route(
        &mut tenant_b,
        "query",
        "workspace.advisory.audit",
        json!({"limit":10}),
    )
    .await;
    assert_eq!(empty_b["aggregate"]["opportunities"], 0);

    let runtime_pool = PgPool::connect(&runtime_url).await.unwrap();
    let mut tx = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('tect.tenant_id',$1,true)")
        .bind(enrollment.tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let visible_configs: i64 = sqlx::query_scalar("SELECT count(*) FROM advisory_workspace_config")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(visible_configs, 1);
    tx.rollback().await.unwrap();
    let all_configs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_workspace_config WHERE tenant_id IN ($1,$2)",
    )
    .bind(enrollment.tenant_id)
    .bind(enrollment_b.tenant_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(all_configs, 2);

    tenant_b.finish().await;
    peer.finish().await;
    client.finish().await;
    daemon.child.start_kill().unwrap();
    let _ = daemon.child.wait().await.unwrap();
}
