#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
mod support;

use recovery_support::{Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{os::unix::fs::PermissionsExt, sync::Arc};
use support::{id, ready_source_candidate, repository, route};
use tect_application::WorkspaceService;
use tect_postgres::{PgScopeAuthoredManifestSupplier, PgScopeAuthorityObserver, PgStore, admin};
use tokio::net::UnixListener;
use uuid::Uuid;

fn authored_draft(source_ref: Uuid, prior: &Value, title: &str) -> Value {
    json!({"boundary":"ongoing","goals":[{
        "identity":{"local":"goal"},"text":"Preserve source evidence",
        "source_ref_id":source_ref,
        "resolution":{"kind":"candidate","reference":{"local":"candidate"}}
    }],"evidence":[],"candidates":[{
        "identity":{"local":"candidate"},"title":title,
        "outcome":"The preview cause is demonstrated",
        "trigger":"Preview differs from settings",
        "delivered_behavior":"A bounded correction is selected",
        "proof":"Direct source evidence is retained",
        "includes":["diagnosis"],"excludes":["deployment"],
        "dependencies":[],"coverage_goals":[{"local":"goal"}],"evidence":[]
    }],"blockers":[],"protected_changes":[],"supersessions":[{
        "candidate_id":prior["id"],"revision":prior["revision"],
        "reason":"Compare this authored option with the prior candidate",
        "replacements":[{"local":"candidate"}]
    }]})
}

async fn authored_scope_set(
    client: &mut Mcp,
    candidate_set: Uuid,
    revision: i64,
    prior: &Value,
) -> Value {
    let inputs = client
        .call(
            "candidate_context",
            json!({
                "candidate_set_id":candidate_set,"view":"inputs","limit":25
            }),
        )
        .await;
    let program = client
        .call(
            "candidate_context",
            json!({
                "candidate_set_id":candidate_set,"view":"program","limit":25
            }),
        )
        .await;
    let mut refs: Vec<Uuid> = inputs["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| id(&item["input"]["source_ref_id"]))
        .collect();
    let source_ref = *refs.first().unwrap();
    refs.extend(
        program["program"]["field_refs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| id(&field["id"])),
    );
    refs.sort_unstable();
    refs.dedup();
    json!({"expected_candidate_set_revision":revision,
        "baseline_key":"baseline","alternatives":[
            {"key":"baseline","kind":"cohesive",
             "draft":authored_draft(source_ref, prior, "Cohesive diagnosis"),
             "covered_source_ref_ids":refs},
            {"key":"partition","kind":"partitioned",
             "draft":authored_draft(source_ref, prior, "Partitioned diagnosis"),
             "covered_source_ref_ids":refs}]})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires disposable PostgreSQL 18.6 and TECT_TEST_*; run with `cargo test -p tect-cli --test scope_advisory_no_call -- --ignored --nocapture`"]
async fn public_scope_advisory_request_audits_disabled_and_optional_skip_without_dispatch() {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    let identity: (String, String, i32, i64, String, i32) = sqlx::query_as(
        "SELECT current_database(),current_user,current_setting('server_version_num')::integer,\
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_control_system()),\
         inet_server_port()",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        identity.0,
        std::env::var("TECT_TEST_EXPECTED_DB_NAME").unwrap()
    );
    assert_eq!(identity.1, "postgres");
    assert_eq!(identity.2, 180_006);
    assert_eq!(
        identity.3.to_string(),
        std::env::var("TECT_TEST_EXPECTED_DB_OID").unwrap()
    );
    assert_eq!(
        identity.4,
        std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").unwrap()
    );
    assert_eq!(
        identity.5.to_string(),
        std::env::var("TECT_TEST_EXPECTED_PG_PORT").unwrap()
    );
    assert_eq!(role, "tect_ci");
    let runtime_identity: (String, String) =
        sqlx::query_as("SELECT current_database(),current_user")
            .fetch_one(&PgPool::connect(&runtime_url).await.unwrap())
            .await
            .unwrap();
    assert_eq!(runtime_identity, (identity.0.clone(), role.clone()));
    admin::migrate(&pool, &role).await.unwrap();

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("scope-advisory-no-call.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-no-call-{}", Uuid::new_v4()));
    let store = PgStore::connect(&runtime, 4).await.unwrap();
    let authority = Arc::new(PgScopeAuthorityObserver::new(
        store.clone(),
        Arc::new(tect_host::StaticCandidateGuidance),
    ));
    let supplier = Arc::new(PgScopeAuthoredManifestSupplier::new(
        store.clone(),
        authority.clone(),
    ));
    let service = Arc::new(WorkspaceService::new_with_scope_sources(
        Arc::new(store),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
        authority,
        supplier,
    ));
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));

    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config_path = root.join("host.json");
    host_file(&config_path, &enrollment.auth);
    let workspace_key = format!("scope-no-call-{}", Uuid::new_v4());
    let native_session = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config_path, &native_session, &workspace_key).await;

    let (candidate_context, prior) = ready_source_candidate(&mut client, &repo).await;
    let candidate_set_id = id(&candidate_context["candidate_set"]["id"]);
    let candidate_revision = candidate_context["candidate_set"]["revision"]
        .as_i64()
        .unwrap();
    let config = route(&mut client, "query", "workspace.advisory.config", json!({})).await;
    assert_eq!(config["revision"], 0);
    assert_eq!(config["mode"], "disabled");
    let workspace_id = id(&config["workspace_id"]);

    let request_id = Uuid::new_v4();
    let request = route(
        &mut client,
        "command",
        "scope.advisory.request",
        json!({"request_id":request_id,"candidate_set_id":candidate_set_id}),
    )
    .await;
    assert_eq!(request["request_id"], request_id.to_string());
    assert_eq!(request["candidate_set_id"], candidate_set_id.to_string());
    assert_eq!(request["state"], "no_call");
    assert_eq!(request["reason"], "workspace_disabled");
    assert_eq!(request["provider_called"], false);

    let audit = route(
        &mut client,
        "query",
        "candidate.advisory.audit",
        json!({"candidate_set_id":candidate_set_id,"limit":10}),
    )
    .await;
    assert_eq!(audit["opportunities"].as_array().unwrap().len(), 1);
    assert_eq!(audit["opportunities"][0]["id"], request["opportunity_id"]);
    assert_eq!(
        audit["opportunities"][0]["work_item_id"],
        candidate_set_id.to_string()
    );
    assert_eq!(
        audit["opportunities"][0]["source_revision"],
        candidate_revision.to_string()
    );
    assert_eq!(audit["opportunities"][0]["state"], "no_call");
    assert_eq!(
        audit["opportunities"][0]["primary_reason"],
        "workspace_disabled"
    );
    assert_eq!(audit["dispatches"].as_array().unwrap().len(), 0);
    assert_eq!(audit["aggregate"]["opportunities"], 1);
    assert_eq!(audit["aggregate"]["opportunities_with_attempts"], 0);
    assert_eq!(audit["aggregate"]["no_call_opportunities"], 1);
    assert_eq!(audit["aggregate"]["authorized_attempts"], 0);
    assert_eq!(audit["aggregate"]["confirmed_sent_attempts"], 0);
    assert_eq!(audit["aggregate"]["send_unknown_attempts"], 0);
    assert_eq!(audit["aggregate"]["proven_unsent_attempts"], 0);
    assert_eq!(
        audit["aggregate"]["no_call_by_reason"][0]["reason"],
        "workspace_disabled"
    );
    assert_eq!(audit["aggregate"]["no_call_by_reason"][0]["count"], 1);

    let configured = route(
        &mut client,
        "command",
        "workspace.advisory.configure",
        json!({"expected_revision":0,"mode":"optional"}),
    )
    .await;
    assert_eq!(configured["workspace_id"], workspace_id.to_string());
    assert_eq!(configured["revision"], 1);
    assert_eq!(configured["mode"], "optional");

    let skipped_id = Uuid::new_v4();
    let skipped = route(
        &mut client,
        "command",
        "scope.advisory.request",
        json!({
            "request_id": skipped_id,
            "candidate_set_id": candidate_set_id,
            "request_preference": "skip"
        }),
    )
    .await;
    assert_eq!(skipped["request_id"], skipped_id.to_string());
    assert_eq!(skipped["state"], "no_call");
    assert_eq!(skipped["reason"], "request_skip");
    assert_eq!(skipped["provider_called"], false);

    let (session_id, actor_id): (Uuid, Uuid) = sqlx::query_as(
        "SELECT s.id,h.principal_id FROM agent_sessions s JOIN hosts h \
         ON h.tenant_id=s.tenant_id AND h.id=s.host_id \
         WHERE s.tenant_id=$1 AND s.native_session_id=$2",
    )
    .bind(enrollment.tenant_id)
    .bind(&native_session)
    .fetch_one(&pool)
    .await
    .unwrap();
    let skipped_audit = route(
        &mut client,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":candidate_set_id,"opportunity_id":skipped["opportunity_id"]}),
    )
    .await;
    let opportunity = &skipped_audit["opportunity"];
    assert_eq!(opportunity["workspace_id"], workspace_id.to_string());
    assert_eq!(opportunity["work_item_id"], candidate_set_id.to_string());
    assert_eq!(
        opportunity["source_revision"],
        candidate_revision.to_string()
    );
    assert_eq!(opportunity["session_id"], session_id.to_string());
    assert_eq!(opportunity["authorized_actor_id"], actor_id.to_string());
    assert_eq!(opportunity["request_key"], skipped_id.to_string());
    assert_eq!(opportunity["config_revision"], 1);
    assert_eq!(opportunity["session_preference"], "use_workspace");
    assert_eq!(opportunity["request_preference"], "skip");
    assert_eq!(opportunity["state"], "no_call");
    assert_eq!(opportunity["primary_reason"], "request_skip");
    assert!(skipped_audit["dispatches"].as_array().unwrap().is_empty());
    assert!(skipped_audit.get("scope_decomposition").is_none());
    let dispatch_attempt_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch \
         WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(id(&skipped["opportunity_id"]))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        dispatch_attempt_rows, 0,
        "explicit skip must persist no dispatch or provider-attempt row"
    );

    let skipped_page = route(
        &mut client,
        "query",
        "candidate.advisory.audit",
        json!({"candidate_set_id":candidate_set_id,"limit":10,"reason":"request_skip"}),
    )
    .await;
    assert_eq!(skipped_page["opportunities"].as_array().unwrap().len(), 1);
    assert_eq!(
        skipped_page["opportunities"][0]["id"],
        skipped["opportunity_id"]
    );
    assert!(skipped_page["dispatches"].as_array().unwrap().is_empty());
    assert_eq!(skipped_page["aggregate"]["no_call_opportunities"], 1);
    assert_eq!(skipped_page["aggregate"]["authorized_attempts"], 0);
    assert_eq!(skipped_page["aggregate"]["confirmed_sent_attempts"], 0);
    assert_eq!(skipped_page["aggregate"]["send_unknown_attempts"], 0);

    let default_preference = route(
        &mut client,
        "query",
        "session.advisory.preference",
        json!({}),
    )
    .await;
    assert_eq!(default_preference["revision"], 0);
    assert_eq!(default_preference["preference"], "use_workspace");
    let committed_skip = route(
        &mut client,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":0,"preference":"skip"}),
    )
    .await;
    assert_eq!(committed_skip["revision"], 1);
    assert_eq!(committed_skip["preference"], "skip");
    let session_request = Uuid::new_v4();
    let session_skipped = route(
        &mut client,
        "command",
        "scope.advisory.request",
        json!({"request_id":session_request,"candidate_set_id":candidate_set_id}),
    )
    .await;
    assert_eq!(session_skipped["state"], "no_call");
    assert_eq!(session_skipped["reason"], "session_skip");
    assert_eq!(session_skipped["provider_called"], false);
    let session_detail = route(
        &mut client,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":candidate_set_id,
            "opportunity_id":session_skipped["opportunity_id"]}),
    )
    .await;
    assert_eq!(
        session_detail["opportunity"]["session_id"],
        session_id.to_string()
    );
    assert_eq!(
        session_detail["opportunity"]["authorized_actor_id"],
        actor_id.to_string()
    );
    assert_eq!(
        session_detail["opportunity"]["work_item_id"],
        candidate_set_id.to_string()
    );
    assert_eq!(
        session_detail["opportunity"]["source_revision"],
        candidate_revision.to_string()
    );
    assert_eq!(
        session_detail["opportunity"]["request_key"],
        session_request.to_string()
    );
    assert_eq!(session_detail["opportunity"]["session_preference"], "skip");
    assert_eq!(
        session_detail["opportunity"]["request_preference"],
        "use_workspace"
    );
    assert_eq!(
        session_detail["opportunity"]["primary_reason"],
        "session_skip"
    );
    assert!(session_detail["dispatches"].as_array().unwrap().is_empty());
    let session_page = route(
        &mut client,
        "query",
        "candidate.advisory.audit",
        json!({"candidate_set_id":candidate_set_id,"limit":10,"reason":"session_skip"}),
    )
    .await;
    assert_eq!(session_page["opportunities"].as_array().unwrap().len(), 1);
    assert_eq!(
        session_page["opportunities"][0]["id"],
        session_skipped["opportunity_id"]
    );
    assert_eq!(session_page["aggregate"]["authorized_attempts"], 0);
    let session_replay = route(
        &mut client,
        "command",
        "scope.advisory.request",
        json!({"request_id":session_request,"candidate_set_id":candidate_set_id}),
    )
    .await;
    assert_eq!(
        session_replay["opportunity_id"],
        session_skipped["opportunity_id"]
    );

    let restored = route(
        &mut client,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":1,"preference":"use_workspace"}),
    )
    .await;
    assert_eq!(restored["revision"], 2);
    let restored_replay = route(
        &mut client,
        "command",
        "scope.advisory.request",
        json!({"request_id":session_request,"candidate_set_id":candidate_set_id}),
    )
    .await;
    assert_eq!(
        restored_replay["opportunity_id"],
        session_skipped["opportunity_id"]
    );
    assert_eq!(restored_replay["reason"], "session_skip");
    let replay_detail = route(
        &mut client,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":candidate_set_id,
            "opportunity_id":restored_replay["opportunity_id"]}),
    )
    .await;
    for field in [
        "session_id",
        "authorized_actor_id",
        "work_item_id",
        "source_revision",
        "request_key",
        "session_preference",
        "request_preference",
        "material_digest",
    ] {
        assert_eq!(
            replay_detail["opportunity"][field], session_detail["opportunity"][field],
            "replay changed {field}"
        );
    }
    assert!(replay_detail["dispatches"].as_array().unwrap().is_empty());
    let authored =
        authored_scope_set(&mut client, candidate_set_id, candidate_revision, &prior).await;
    let eligible_id = Uuid::new_v4();
    let eligible = route(
        &mut client,
        "command",
        "scope.advisory.request",
        json!({"request_id":eligible_id,"candidate_set_id":candidate_set_id,
            "authored_scope_set":authored}),
    )
    .await;
    assert_eq!(eligible["state"], "no_call", "{eligible}");
    assert_eq!(eligible["reason"], "capability_unavailable", "{eligible}");
    assert_eq!(eligible["provider_called"], false);
    let eligible_detail = route(
        &mut client,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":candidate_set_id,
            "opportunity_id":eligible["opportunity_id"]}),
    )
    .await;
    assert_eq!(
        eligible_detail["opportunity"]["session_id"],
        session_id.to_string()
    );
    assert_eq!(
        eligible_detail["opportunity"]["authorized_actor_id"],
        actor_id.to_string()
    );
    assert_eq!(
        eligible_detail["opportunity"]["work_item_id"],
        candidate_set_id.to_string()
    );
    assert_eq!(
        eligible_detail["opportunity"]["source_revision"],
        candidate_revision.to_string()
    );
    assert_eq!(
        eligible_detail["opportunity"]["request_key"],
        eligible_id.to_string()
    );
    assert_eq!(eligible_detail["opportunity"]["config_revision"], 1);
    assert_eq!(
        eligible_detail["opportunity"]["session_preference"],
        "use_workspace"
    );
    assert_eq!(
        eligible_detail["opportunity"]["request_preference"],
        "use_workspace"
    );
    assert_eq!(
        eligible_detail["opportunity"]["primary_reason"],
        "capability_unavailable"
    );
    assert!(eligible_detail["dispatches"].as_array().unwrap().is_empty());
    let eligible_page = route(
        &mut client,
        "query",
        "candidate.advisory.audit",
        json!({"candidate_set_id":candidate_set_id,"limit":10,
            "reason":"capability_unavailable"}),
    )
    .await;
    assert_eq!(eligible_page["opportunities"].as_array().unwrap().len(), 1);
    assert_eq!(
        eligible_page["opportunities"][0]["id"],
        eligible["opportunity_id"]
    );
    assert_eq!(eligible_page["aggregate"]["authorized_attempts"], 0);
    assert_eq!(eligible_page["aggregate"]["send_unknown_attempts"], 0);
    let dispatch_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(dispatch_count, 0);

    let history: Vec<(i64, String)> = sqlx::query_as(
        "SELECT revision,mode FROM advisory_workspace_config_history \
         WHERE tenant_id=$1 AND workspace_id=$2 ORDER BY revision",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(history, [(0, "disabled".into()), (1, "optional".into())]);
    let preference_history: Vec<(i64, String)> = sqlx::query_as(
        "SELECT revision,preference FROM session_advisory_preference_history \
         WHERE tenant_id=$1 AND workspace_id=$2 AND session_id=$3 ORDER BY revision",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(session_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        preference_history,
        [
            (0, "use_workspace".into()),
            (1, "skip".into()),
            (2, "use_workspace".into())
        ]
    );

    client.finish().await;
    server.abort();
    let _ = server.await;
}
