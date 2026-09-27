//! Public native MCP proof for declared Matrix requirements on synthetic ancestry.
//! The owner response reference below is a fixture marker, not Tony's consent.
//! Ignored: writes only after the exact disposable PG18.6 identity guard passes.
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use recovery_support::{Daemon, Mcp, host_file, private_temp};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgConnectOptions};
use std::str::FromStr;
use support::{
    id, open_slice, ready_source_candidate, repository, review, route, route_error, save,
};
use tect_postgres::admin;
use uuid::Uuid;

const OWNER_RESPONSE: &str = "fixture:synthetic-owner-response";
const SYSTEM_ID: &str = "7689676854994613066";
const DATABASE_OID: i64 = 16385;

async fn disposable_pair() -> (PgPool, String) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    assert_eq!(
        std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").as_deref(),
        Ok(SYSTEM_ID)
    );
    assert_eq!(
        std::env::var("TECT_TEST_EXPECTED_DB_OID").as_deref(),
        Ok("16385")
    );
    assert_eq!(
        std::env::var("TECT_TEST_RUNTIME_ROLE").as_deref(),
        Ok("tect_ci")
    );
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let admin_options = PgConnectOptions::from_str(&admin_url).unwrap();
    let runtime_options = PgConnectOptions::from_str(&runtime_url).unwrap();
    for (options, user) in [(&admin_options, "postgres"), (&runtime_options, "tect_ci")] {
        assert_eq!(options.get_username(), user);
        assert_eq!(options.get_database(), Some("tect_test"));
        assert_eq!(options.get_host(), "127.0.0.1");
        assert_eq!(options.get_port(), 64775);
        assert!(options.get_socket().is_none());
    }
    let pool = PgPool::connect_with(admin_options).await.unwrap();
    let identity: (i32, String, String, i64, String) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),current_user,\
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_control_system())",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        identity,
        (
            180006,
            "tect_test".into(),
            "postgres".into(),
            DATABASE_OID,
            SYSTEM_ID.into()
        )
    );
    let runtime_pool = PgPool::connect_with(runtime_options).await.unwrap();
    let runtime_identity: (String, String, i64) = sqlx::query_as(
        "SELECT current_database(),current_user,(SELECT oid::bigint FROM pg_database WHERE datname=current_database())",
    ).fetch_one(&runtime_pool).await.unwrap();
    assert_eq!(
        runtime_identity,
        ("tect_test".into(), "tect_ci".into(), DATABASE_OID)
    );
    (pool, runtime_url)
}

fn locator_program(program: Uuid) -> Value {
    json!({"level":"program","program_id":program})
}
fn locator_scope(program: Uuid, scope: Uuid) -> Value {
    json!({"level":"scope","program_id":program,"scope_id":scope})
}
fn locator_work(program: Uuid, scope: Uuid, set: Uuid, work: Uuid, revision: i64) -> Value {
    json!({"level":"slice","program_id":program,"scope_id":scope,
        "candidate_set_id":set,"work_candidate_id":work,"expected_work_revision":revision})
}
fn mode(value: &str) -> Value {
    json!({"operation":"set","value":{"kind":"mode","value":value}})
}
fn proposed(locator: Value, revision: u64, patches: Vec<Value>) -> Value {
    json!({"request_id":Uuid::new_v4(),"locator":locator,
        "expected_context_revision":revision,"patches":patches})
}
async fn get(client: &mut Mcp, locator: Value) -> Value {
    route(
        client,
        "query",
        "engineering.matrix.context.effective.get",
        json!({"locator":locator}),
    )
    .await
}
async fn confirm(client: &mut Mcp, locator: Value, proposal: &Value) -> Value {
    route(
        client,
        "command",
        "engineering.matrix.context.confirm",
        json!({
            "request_id":Uuid::new_v4(),"locator":locator,
            "proposal_revision":proposal["proposal"]["revision"],
            "proposal_digest":proposal["proposal"]["digest"],
            "owner_response_ref":OWNER_RESPONSE
        }),
    )
    .await
}
fn mode_value(effective: &Value) -> &Value {
    &effective["values"]["mode"]["value"]
}
fn assert_code(result: &Value, allowed: &[&str]) {
    let code = result["error"]["code"].as_str().unwrap();
    assert!(allowed.contains(&code), "unexpected {code}: {result}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "writes only exact owned disposable PostgreSQL 18.6 fixture"]
async fn public_context_inherits_once_and_work_override_is_local() {
    let (pool, runtime_url) = disposable_pair().await;
    admin::migrate(&pool, "tect_ci").await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("context.sock");
    let daemon = Daemon::start(&runtime_url, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let host = root.join("host.json");
    host_file(&host, &enrollment.auth);
    let workspace_key = format!("matrix-context-{}", Uuid::new_v4());
    let mut owner = Mcp::start(&socket, &host, &Uuid::new_v4().to_string(), &workspace_key).await;
    let (source, candidate) = ready_source_candidate(&mut owner, &repo).await;
    let opened_workspace = owner.call("open_workspace", json!({})).await;
    let workspace_id = id(&opened_workspace["workspace"]["id"]);
    let verifier = admin::prepare_verifier_enrollment(&pool, enrollment.tenant_id, workspace_id)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let verifier_host = root.join("verifier.json");
    host_file(&verifier_host, &verifier.auth);
    let mut non_owner = Mcp::start(
        &socket,
        &verifier_host,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    non_owner.call("open_workspace", json!({})).await;
    let program: Uuid =
        sqlx::query_scalar("SELECT program_id FROM scope_candidate_sets WHERE id=$1")
            .bind(id(&source["candidate_set"]["id"]))
            .fetch_one(&pool)
            .await
            .unwrap();
    let program_locator = locator_program(program);
    let before = get(&mut owner, program_locator.clone()).await;
    assert!(before["values"].as_object().unwrap().is_empty());

    let request = proposed(program_locator.clone(), 0, vec![mode("demo")]);
    assert_code(
        &route_error(
            &mut non_owner,
            "command",
            "engineering.matrix.context.propose",
            request.clone(),
        )
        .await,
        &["forbidden"],
    );
    let proposal = route(
        &mut owner,
        "command",
        "engineering.matrix.context.propose",
        request.clone(),
    )
    .await;
    assert_eq!(proposal["proposal"]["revision"], 1);
    let replay = route(
        &mut owner,
        "command",
        "engineering.matrix.context.propose",
        request.clone(),
    )
    .await;
    assert_eq!(replay["proposal"], proposal["proposal"]);
    assert!(
        get(&mut owner, program_locator.clone()).await["values"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    let mut changed = request.clone();
    changed["patches"] = json!([mode("mvp")]);
    assert_code(
        &route_error(
            &mut owner,
            "command",
            "engineering.matrix.context.propose",
            changed,
        )
        .await,
        &["input_conflict"],
    );
    let stale = proposed(program_locator.clone(), 0, vec![mode("mvp")]);
    assert_code(
        &route_error(
            &mut owner,
            "command",
            "engineering.matrix.context.propose",
            stale,
        )
        .await,
        &["stale_revision"],
    );
    let wrong = json!({"request_id":Uuid::new_v4(),"locator":program_locator,
        "proposal_revision":1,"proposal_digest":"0".repeat(64),"owner_response_ref":OWNER_RESPONSE});
    assert_code(
        &route_error(
            &mut owner,
            "command",
            "engineering.matrix.context.confirm",
            wrong,
        )
        .await,
        &["invalid_arguments"],
    );
    let confirmed = confirm(&mut owner, locator_program(program), &proposal).await;
    assert_eq!(
        confirmed["confirmation"]["proposal_digest"],
        proposal["proposal"]["digest"]
    );
    let program_effective = get(&mut owner, locator_program(program)).await;
    assert_eq!(
        mode_value(&program_effective),
        &json!({"kind":"mode","value":"demo"})
    );
    assert_eq!(
        program_effective["values"]["mode"]["source"]["anchor"],
        locator_program(program)
    );
    let unauthorized_confirmation = json!({"request_id":Uuid::new_v4(),"locator":locator_program(program),
        "proposal_revision":proposal["proposal"]["revision"],
        "proposal_digest":proposal["proposal"]["digest"],"owner_response_ref":OWNER_RESPONSE});
    assert_code(
        &route_error(
            &mut non_owner,
            "command",
            "engineering.matrix.context.confirm",
            unauthorized_confirmation,
        )
        .await,
        &["forbidden"],
    );

    let opened = route(
        &mut owner,
        "command",
        "scope.open",
        json!({
            "request_id":Uuid::new_v4(),"candidate_set_id":source["candidate_set"]["id"],
            "candidate_set_revision":source["candidate_set"]["revision"],
            "candidate_snapshot_id":source["snapshot"]["id"],
            "candidate_id":candidate["id"],"candidate_revision":candidate["revision"]
        }),
    )
    .await;
    let planning = &opened["created"]["planning"];
    let scope = id(&planning["scope"]["id"]);
    let scope_locator = locator_scope(program, scope);
    assert_eq!(
        mode_value(&get(&mut owner, scope_locator.clone()).await),
        mode_value(&program_effective)
    );

    let draft = json!({"coverage_summary":"One bounded synthetic work item","nodes":[{
        "kind":"work","identity":{"local":"work"},"title":"Demonstrate fixture behavior",
        "outcome":"Fixture behavior observed","includes":["synthetic proof"],"excludes":["deployment"],
        "dependencies":[],"proof":["Public MCP result"],"pipeline":"slice.debug-root-cause",
        "pipeline_reason":"Synthetic diagnosis","source_result_ids":[]
    }],"supersessions":[]});
    let saved = save(&mut owner, planning, draft).await;
    let work = saved["draft"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "work")
        .unwrap()
        .clone();
    let set = id(&saved["candidate_set"]["id"]);
    let work_locator = locator_work(
        program,
        scope,
        set,
        id(&work["id"]),
        work["revision"].as_i64().unwrap(),
    );
    assert_eq!(
        mode_value(&get(&mut owner, work_locator.clone()).await),
        mode_value(&program_effective)
    );
    let pending_request = proposed(work_locator.clone(), 0, vec![mode("mvp")]);
    let work_proposal = route(
        &mut owner,
        "command",
        "engineering.matrix.context.propose",
        pending_request,
    )
    .await;
    assert_eq!(
        mode_value(&get(&mut owner, work_locator.clone()).await),
        mode_value(&program_effective)
    );
    confirm(&mut owner, work_locator.clone(), &work_proposal).await;
    let work_effective = get(&mut owner, work_locator.clone()).await;
    assert_eq!(
        mode_value(&work_effective),
        &json!({"kind":"mode","value":"mvp"})
    );
    assert_eq!(
        work_effective["values"]["mode"]["source"]["anchor"]["work_candidate_id"],
        work["id"]
    );
    assert_eq!(
        mode_value(&get(&mut owner, scope_locator).await),
        mode_value(&program_effective)
    );

    let reviewed = review(&mut owner, &saved).await;
    let native_opened = route(
        &mut owner,
        "command",
        "slice.open",
        open_slice(&reviewed, &work, Uuid::new_v4()),
    )
    .await;
    let slice_id = id(&native_opened["created"]["id"]);
    let opened_effective = get(
        &mut owner,
        json!({"level":"opened_slice","slice_id":slice_id}),
    )
    .await;
    assert_eq!(opened_effective["values"], work_effective["values"]);

    let wrong_program = locator_work(
        Uuid::new_v4(),
        scope,
        set,
        id(&work["id"]),
        work["revision"].as_i64().unwrap(),
    );
    assert_code(
        &route_error(
            &mut owner,
            "query",
            "engineering.matrix.context.effective.get",
            json!({"locator":wrong_program}),
        )
        .await,
        &["not_found", "input_conflict"],
    );
    let after = get(&mut owner, work_locator).await;
    assert_eq!(after, work_effective);
    non_owner.finish().await;
    owner.finish().await;
    drop(daemon);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "writes only exact owned disposable PostgreSQL 18.6 fixture"]
async fn public_context_rejects_caller_authorship_and_operating_facts() {
    let (pool, runtime_url) = disposable_pair().await;
    admin::migrate(&pool, "tect_ci").await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("context-denials.sock");
    let daemon = Daemon::start(&runtime_url, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let host = root.join("host.json");
    host_file(&host, &enrollment.auth);
    let mut owner = Mcp::start(
        &socket,
        &host,
        &Uuid::new_v4().to_string(),
        &format!("matrix-context-denials-{}", Uuid::new_v4()),
    )
    .await;
    let (source, _) = ready_source_candidate(&mut owner, &repo).await;
    let program: Uuid =
        sqlx::query_scalar("SELECT program_id FROM scope_candidate_sets WHERE id=$1")
            .bind(id(&source["candidate_set"]["id"]))
            .fetch_one(&pool)
            .await
            .unwrap();
    let locator = locator_program(program);
    let valid = proposed(locator.clone(), 0, vec![mode("demo")]);
    let mut denied = Vec::new();
    let mut human = valid.clone();
    human["human_confirmed"] = json!(true);
    denied.push(human);
    let mut facts = valid.clone();
    facts["patches"] =
        json!([{"operation":"set","value":{"kind":"operational_facts","value":"trusted"}}]);
    denied.push(facts);
    let mut intent = valid.clone();
    intent["patches"] = json!([{"operation":"set","value":{"kind":"intent","value":{"kind":"other","description":"fixture","operating_fact":"yes"}}}]);
    denied.push(intent);
    let mut oversized = valid.clone();
    oversized["patches"] =
        json!([{"operation":"set","value":{"kind":"promised_proof","value":"x".repeat(257)}}]);
    denied.push(oversized);
    for request in denied {
        assert_code(
            &route_error(
                &mut owner,
                "command",
                "engineering.matrix.context.propose",
                request,
            )
            .await,
            &["invalid_arguments"],
        );
    }
    assert!(
        get(&mut owner, locator).await["values"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    owner.finish().await;
    drop(daemon);
}
