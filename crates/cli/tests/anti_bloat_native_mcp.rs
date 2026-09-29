#[path = "anti_bloat_native_mcp/audit.rs"]
mod audit;
#[path = "anti_bloat_native_mcp/process.rs"]
mod process;
#[allow(dead_code)]
mod recovery_support;
#[path = "anti_bloat_native_mcp/setup.rs"]
mod setup;
#[path = "anti_bloat_native_mcp/source.rs"]
mod source;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;
use recovery_support::{Mcp, host_file, private_temp};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{os::unix::fs::PermissionsExt, sync::Arc};
use support::{id, repository};
use tect_application::{Store, TransactionMode, WorkspaceService};
use tect_postgres::{PgStore, admin};
use tokio::net::{TcpListener, UnixListener};
use uuid::Uuid;
type SealedAudit = (
    Vec<u8>,
    String,
    Vec<u8>,
    String,
    Option<i32>,
    String,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<bool>,
    Option<Value>,
);

// This guard is intentionally tied to the explicitly owned disposable cluster.
async fn identity(pool: &PgPool) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let row:(String,i64,String,i64)=sqlx::query_as("SELECT current_database(),d.oid::bigint,(SELECT system_identifier::text FROM pg_control_system()),(SELECT max(version) FROM _sqlx_migrations) FROM pg_database d WHERE datname=current_database()")
        .fetch_one(pool).await.unwrap();
    let expected = (
        std::env::var("TECT_TEST_EXPECTED_DB_NAME").unwrap(),
        std::env::var("TECT_TEST_EXPECTED_DB_OID")
            .unwrap()
            .parse::<i64>()
            .unwrap(),
        std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").unwrap(),
        sqlx::migrate!("../postgres/migrations")
            .iter()
            .last()
            .unwrap()
            .version,
    );
    assert!(expected.0.starts_with("tect_s04_live_"));
    assert_eq!(row, expected);
}
async fn call(pool: &PgPool, client: &mut Mcp, kind: &str, route: &str, params: Value) -> Value {
    identity(pool).await;
    support::route(client, kind, route, params).await
}

async fn policy(
    pool: &PgPool,
    store: &PgStore,
    auth: &tect_domain::HostAuth,
    tenant: Uuid,
    workspace: Uuid,
    actor: Uuid,
) -> Value {
    use tect_domain::{AdvisoryBudgetCeilings, AdvisoryBudgetPolicy};
    let key = Ed25519KeyPair::from_seed_unchecked(&[93u8; 32]).unwrap();
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let keys = json!([{"workspace_id":workspace,"owner_id":actor,"public_key_hex":hex(key.public_key().as_ref())}]);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let ceiling = AdvisoryBudgetCeilings {
        provider_calls: 10,
        input_tokens: 1000,
        output_tokens: 1000,
        request_utf8_bytes: 1_000_000,
        elapsed_monotonic_ms: 120_000,
        retry_dispatches: 1,
    };
    let id = Uuid::new_v4();
    let from = now - 60_000;
    let until = now + 600_000;
    let unsigned = AdvisoryBudgetPolicy::new(
        id,
        1,
        AdvisoryBudgetPolicy::digest_for(id, 1, from, until, ceiling),
        from,
        until,
        ceiling,
        actor,
        "0".repeat(128),
    )
    .unwrap();
    let signed = AdvisoryBudgetPolicy::new(
        id,
        1,
        unsigned.digest().into(),
        from,
        until,
        ceiling,
        actor,
        hex(key
            .sign(&unsigned.approval_signing_message(workspace).unwrap())
            .as_ref()),
    )
    .unwrap();
    identity(pool).await;
    let mut unit = store.begin(TransactionMode::ReadWrite).await.unwrap();
    unit.authenticate(auth).await.unwrap();
    unit.set_tenant(tenant).await.unwrap();
    unit.advisory_budget_policy_store()
        .unwrap()
        .install_budget_policy(workspace, &signed)
        .await
        .unwrap();
    unit.commit().await.unwrap();
    keys
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a fresh exact-owned disposable PostgreSQL 18 database"]
async fn historical_s04_review_survives_0104_to_0113_public_get_and_run_fails_closed() {
    use sqlx::postgres::PgConnectOptions;
    use std::{fs, str::FromStr};

    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let database = std::env::var("TECT_TEST_EXPECTED_DB_NAME").unwrap();
    assert!(database.starts_with("tect_s04_upgrade_"));
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let options = PgConnectOptions::from_str(&admin_url).unwrap();
    let runtime_options = PgConnectOptions::from_str(&runtime_url).unwrap();
    assert_eq!(options.get_host(), "127.0.0.1");
    assert_eq!(options.get_database(), Some(database.as_str()));
    assert_eq!(options.get_username(), "postgres");
    assert_eq!(runtime_options.get_host(), "127.0.0.1");
    assert_eq!(runtime_options.get_database(), Some(database.as_str()));
    assert_eq!(runtime_options.get_username(), "tect_ci");
    assert_eq!(runtime_options.get_port(), options.get_port());
    let pool = PgPool::connect(&admin_url).await.unwrap();
    let actual: (i32, String, i64, String, bool) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(), \
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()), \
         (SELECT system_identifier::text FROM pg_control_system()), \
         to_regclass('public._sqlx_migrations') IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(actual.0, 180006);
    assert_eq!(actual.1, database);
    assert_eq!(
        actual.2,
        std::env::var("TECT_TEST_EXPECTED_DB_OID")
            .unwrap()
            .parse::<i64>()
            .unwrap()
    );
    assert_eq!(
        actual.3,
        std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").unwrap()
    );
    assert!(actual.4, "fixture must be empty before staged migration");

    let staged = tempfile::tempdir().unwrap();
    let sources = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../postgres/migrations");
    let mut copied = 0;
    for entry in fs::read_dir(sources).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let name = name.to_str().unwrap();
        let version: i64 = name.get(..4).unwrap().parse().unwrap();
        if version <= 104 {
            fs::copy(entry.path(), staged.path().join(name)).unwrap();
            copied += 1;
        }
    }
    assert_eq!(copied, 104);
    sqlx::migrate::Migrator::new(staged.path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    let ledger: (i64, Option<i64>) =
        sqlx::query_as("SELECT count(*),max(version) FROM _sqlx_migrations WHERE success")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(ledger, (104, Some(104)));

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let tenant = enrolled.tenant_id;
    let actor: Uuid = sqlx::query_scalar("SELECT principal_id FROM hosts WHERE id=$1")
        .bind(enrolled.auth.host_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let workspace = Uuid::new_v4();
    let workspace_key = format!("s04-upgrade-{}", Uuid::new_v4());
    let session = Uuid::new_v4();
    let native_session = Uuid::new_v4().to_string();
    let program = Uuid::new_v4();
    let candidate = Uuid::new_v4();
    let snapshot = Uuid::new_v4();
    let opportunity = Uuid::new_v4();
    let review = Uuid::new_v4();
    let d = "a".repeat(64);
    let b = "b".repeat(64);
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(tenant)
        .bind(&workspace_key)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
        .bind(tenant)
        .bind(workspace)
        .bind(actor)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
        .bind(session).bind(tenant).bind(enrolled.auth.host_id).bind(workspace)
        .bind(&native_session).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO programs(id,tenant_id,workspace_id,status,revision,name,intent,basis,boundaries,constraints,success,current_step,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,'open',4,'Legacy program','Preserve intent','Legacy evidence','One Scope','No adjacent work','Review survives','ready',2,2,4096)")
        .bind(program).bind(tenant).bind(workspace).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,origin_result,revision,status,boundary,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,$4,$5,'legacy planning','{}'::jsonb,'{}'::jsonb,2,'review_required','finite',1,1,4096)")
        .bind(candidate).bind(tenant).bind(workspace).bind(program).bind(Uuid::new_v4())
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,'legacy source')")
        .bind(tenant).bind(workspace).bind(&d).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) VALUES($1,$2,$3,$4,1,4,2,1,$5,'{}'::uuid[],$5,'legacy-method','1',$5,'legacy method','[]'::jsonb,'1',$5,'[]'::jsonb)")
        .bind(snapshot).bind(tenant).bind(workspace).bind(candidate).bind(&d)
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,session_id,authorized_actor_id,source_revision,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,$5,$6,'2','scope_decomposition','scope.decomposition.before_selection',0,'use_workspace','use_workspace','fixture',$7,$8,'no_call','workspace_disabled')")
        .bind(opportunity).bind(tenant).bind(workspace).bind(candidate).bind(session).bind(actor)
        .bind(Uuid::new_v4().to_string()).bind(&d).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_scope_source_snapshot(tenant_id,workspace_id,opportunity_id,candidate_set_id,config_revision,opportunity_material_digest,candidate_set_revision,snapshot_id,source_digest,aggregate_schema,aggregate_payload) VALUES($1,$2,$3,$4,0,$5,2,$6,$5,'tect.scope-source-obligations/1','{}'::jsonb)")
        .bind(tenant).bind(workspace).bind(opportunity).bind(candidate).bind(&d).bind(snapshot)
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_scope_manifest(tenant_id,workspace_id,opportunity_id,candidate_set_id,source_digest,constructor_id,constructor_version,constructor_digest,baseline_alternative_id,eligible_set_digest,whole_set_digest,aggregate_schema,aggregate_payload) VALUES($1,$2,$3,$4,$5,'legacy-constructor','1',$5,$5,$5,$5,'tect.scope-constructor-manifest/2','{}'::jsonb)")
        .bind(tenant).bind(workspace).bind(opportunity).bind(candidate).bind(&d)
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_anti_bloat_bindings(tenant_id,workspace_id,candidate_set_id,opportunity_id,candidate_set_revision,source_digest,dependency_digest,obligation_links,mandatory_policy_obligation_ids,provenance) VALUES($1,$2,$3,$4,2,$5,$6,'[]'::jsonb,'[]'::jsonb,'historical source')")
        .bind(tenant).bind(workspace).bind(candidate).bind(opportunity).bind(&d).bind(&b)
        .execute(&pool).await.unwrap();
    let input = json!({"manifest":{"constructor":{"id":"legacy-constructor","version":"1","digest":d},"source":{"candidate_set_id":candidate,"candidate_set_revision":2,"snapshot_id":snapshot,"input_cursor":1,"program_id":program,"program_revision":4,"program_latest_input":2,"planning_latest_input":1,"selected_sources_digest":d,"method_revision":"1","method_digest":d,"registry_revision":"1","registry_digest":d,"inputs":[],"digest":d},"obligations":[],"emitted":[],"rejected":[],"baseline_id":d,"ordered_ids":[],"eligible_set_digest":d,"whole_set_digest":d},"selected_id":d,"selected_revision":2,"graph_provenance":"historical source","dependency_digest":b,"obligation_links":[],"non_goal_source_obligation_ids":[],"mandatory_policy_obligation_ids":[],"protected_obligations":[],"protected_obligations_digest":d});
    let historical_review = json!({"source_digest":d,"whole_set_digest":d,"material_digest":b,"candidate_set_id":candidate,"plan_revision":2,"dependency_digest":b,"protected_obligations_digest":d,"selected_id":d,"findings":[]});
    serde_json::from_value::<tect_domain::AntiBloatInput>(input.clone()).unwrap();
    serde_json::from_value::<tect_domain::AntiBloatReview>(historical_review.clone()).unwrap();
    sqlx::query("INSERT INTO scope_anti_bloat_reviews(tenant_id,workspace_id,review_id,candidate_set_id,candidate_set_revision,actor_id,input_payload,review_payload,state,eligible_ids) VALUES($1,$2,$3,$4,2,$5,$6,$7,'no_eligible','[]'::jsonb)")
        .bind(tenant).bind(workspace).bind(review).bind(candidate).bind(actor).bind(input).bind(historical_review)
        .execute(&pool).await.unwrap();
    let before: (Value, Value, String, Vec<u8>) = sqlx::query_as(
        "SELECT input_payload,review_payload,state,convert_to(input_payload::text || review_payload::text,'UTF8') FROM scope_anti_bloat_reviews WHERE review_id=$1")
        .bind(review).fetch_one(&pool).await.unwrap();

    admin::migrate(&pool, "tect_ci").await.unwrap();
    let after: (Value, Value, String, Vec<u8>, Option<Uuid>) = sqlx::query_as(
        "SELECT input_payload,review_payload,state,convert_to(input_payload::text || review_payload::text,'UTF8'),origin_session_id FROM scope_anti_bloat_reviews WHERE review_id=$1")
        .bind(review).fetch_one(&pool).await.unwrap();
    assert_eq!(
        (&after.0, &after.1, &after.2, &after.3),
        (&before.0, &before.1, &before.2, &before.3)
    );
    assert_eq!(after.4, None);
    let count: (i64, Option<i64>) =
        sqlx::query_as("SELECT count(*),max(version) FROM _sqlx_migrations WHERE success")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, (113, Some(113)));

    let socket = root.join("s04-upgrade.sock");
    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
    let service = Arc::new(WorkspaceService::new(
        Arc::new(store),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    ));
    let listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    let owner_file = root.join("owner.json");
    host_file(&owner_file, &enrolled.auth);
    let mut owner = Mcp::start(&socket, &owner_file, &native_session, &workspace_key).await;
    let got = support::route(
        &mut owner,
        "query",
        "scope.anti_bloat.get",
        json!({"review_id":review}),
    )
    .await;
    assert_eq!(id(&got["review_id"]), review);
    let denied = support::route_error(
        &mut owner,
        "command",
        "scope.anti_bloat.run",
        json!({"review_id":review}),
    )
    .await;
    assert_eq!(denied["error"]["code"], "input_conflict");
    let attempts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM scope_anti_bloat_budget_reservations WHERE review_id=$1),(SELECT count(*) FROM scope_anti_bloat_budget_consumptions WHERE review_id=$1),(SELECT count(*) FROM advisory_call_audit WHERE workspace_id=$2 AND capability='anti_bloat')")
        .bind(review).bind(workspace).fetch_one(&pool).await.unwrap();
    assert_eq!(attempts, (0, 0, 0));
    let final_row: (Value, Value, String, Vec<u8>, Option<Uuid>) = sqlx::query_as("SELECT input_payload,review_payload,state,convert_to(input_payload::text || review_payload::text,'UTF8'),origin_session_id FROM scope_anti_bloat_reviews WHERE review_id=$1")
        .bind(review).fetch_one(&pool).await.unwrap();
    assert_eq!(final_row, after);
    owner.finish().await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires the explicitly owned disposable PostgreSQL 18 fixture"]
async fn native_anti_bloat_public_mcp_is_sealed_once_and_default_disabled() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap())
        .await
        .unwrap();
    identity(&pool).await;
    for (enabled, status, abstain, duplicate, partial, expected) in [
        (true, 200, false, false, 0, "ranked"),
        (true, 200, true, false, 0, "provider_abstained"),
        (true, 500, false, false, 0, "invalid_response"),
        (true, 200, false, true, 0, "invalid_response"),
        (true, 200, false, false, 1, "invalid_response"),
        (true, 200, false, false, 2, "invalid_response"),
        (false, 200, false, false, 0, "no_call"),
        (true, 200, false, false, 0, "no_call"),
    ] {
        let temp = private_temp();
        let root = temp.path().canonicalize().unwrap();
        let repo = root.join("source");
        repository(&repo);
        let socket = root.join("seed.sock");
        let store = PgStore::connect(&runtime, 4).await.unwrap();
        let service = Arc::new(WorkspaceService::new(
            Arc::new(store.clone()),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        ));
        let listener = UnixListener::bind(&socket).unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
        let server = tokio::spawn(tect_host::serve(listener, service));
        identity(&pool).await;
        let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
            .await
            .unwrap();
        let config = root.join("host.json");
        host_file(&config, &enrollment.auth);
        let native = Uuid::new_v4().to_string();
        let key = Uuid::new_v4().to_string();
        let mut client = Mcp::start(&socket, &config, &native, &key).await;
        let opened = call(&pool, &mut client, "command", "workspace.open", json!({})).await;
        let workspace = id(&opened["workspace"]["id"]);
        call(&pool,&mut client,"command","workspace.advisory.configure",json!({"expected_revision":0,"mode":"optional","provider_profile_ref":{"id":if enabled && expected=="no_call" {"different-profile"} else {"fixture-anti-bloat"}},"model_configuration":{"model":"fixture-choice-model"}})).await;
        let actor: Uuid = sqlx::query_scalar("SELECT principal_id FROM hosts WHERE id=$1")
            .bind(enrollment.auth.host_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        let (candidate, revision) = setup::selected_rankable(
            &pool,
            &store,
            &enrollment.auth,
            enrollment.tenant_id,
            actor,
            workspace,
            &native,
            &mut client,
            &repo,
        )
        .await;
        let keys = policy(
            &pool,
            &store,
            &enrollment.auth,
            enrollment.tenant_id,
            workspace,
            actor,
        )
        .await;
        let prepared = call(
            &pool,
            &mut client,
            "command",
            "scope.anti_bloat.prepare",
            json!({"candidate_set_id":candidate,"expected_revision":revision}),
        )
        .await;
        assert_eq!(prepared["state"]["status"], "prepared", "{prepared}");
        let review = id(&prepared["review_id"]);
        let required = prepared["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["rankable"] == false)
            .unwrap();
        let draft: Value = sqlx::query_scalar(
            "SELECT payload FROM scope_candidate_drafts WHERE candidate_set_id=$1 AND set_revision=$2")
            .bind(candidate).bind(revision).fetch_one(&pool).await.unwrap();
        let required_candidate = draft["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == required["candidate_id"])
            .unwrap();
        let rejected = support::route_error(
            &mut client,
            "command",
            "scope.anti_bloat.apply",
            json!({"review_id":review,"finding_id":required["id"],"disposition":"narrow",
                "delta":{"candidate_set_id":candidate,"expected_revision":revision,
                    "idempotency_key":format!("reject-required-{review}"),"operations":[
                        {"operation":"candidate.remove","candidate_id":required["candidate_id"],
                         "expected_revision":required_candidate["revision"]}]}}),
        )
        .await;
        assert_eq!(rejected["error"]["code"], "input_conflict");
        let unchanged: i64 =
            sqlx::query_scalar("SELECT revision FROM scope_candidate_sets WHERE id=$1")
                .bind(candidate)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(unchanged, revision);
        client.finish().await;
        server.abort();
        drop(server);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
        let native_socket = root.join("native.sock");
        let mut daemon = process::daemon(
            &runtime,
            &native_socket,
            enabled.then_some(endpoint.as_str()),
            &keys,
        )
        .await;
        let mut client = Mcp::start(&native_socket, &config, &native, &key).await;
        if enabled && status == 200 && !abstain && !duplicate && partial == 0 {
            let request_skip = call(
                &pool,
                &mut client,
                "command",
                "scope.anti_bloat.prepare",
                json!({"candidate_set_id":candidate,"expected_revision":revision,
                    "request_preference":"skip"}),
            )
            .await;
            assert_eq!(
                request_skip["state"],
                json!({"status":"no_call","reason":"skipped"})
            );
            assert_eq!(request_skip["request_preference"], "skip");
            let other_native = Uuid::new_v4().to_string();
            let mut other = Mcp::start(&native_socket, &config, &other_native, &key).await;
            call(&pool, &mut other, "command", "workspace.open", json!({})).await;
            let foreign = support::route_error(
                &mut other,
                "command",
                "scope.anti_bloat.run",
                json!({"review_id":review}),
            )
            .await;
            assert_eq!(foreign["error"]["code"], "input_conflict", "{foreign}");
            other.finish().await;

            let later_skip = call(
                &pool,
                &mut client,
                "command",
                "scope.anti_bloat.prepare",
                json!({"candidate_set_id":candidate,"expected_revision":revision}),
            )
            .await;
            let later_skip_id = id(&later_skip["review_id"]);
            assert_eq!(later_skip["state"]["status"], "prepared");
            call(
                &pool,
                &mut client,
                "command",
                "session.advisory.preference.set",
                json!({"expected_revision":0,"preference":"skip"}),
            )
            .await;
            let initial_skip = call(
                &pool,
                &mut client,
                "command",
                "scope.anti_bloat.prepare",
                json!({"candidate_set_id":candidate,"expected_revision":revision}),
            )
            .await;
            assert_eq!(
                initial_skip["state"],
                json!({"status":"no_call","reason":"session_skip"})
            );
            assert_eq!(initial_skip["session_preference"], "skip");
            let initial_skip_id = id(&initial_skip["review_id"]);
            let no_send = call(
                &pool,
                &mut client,
                "command",
                "scope.anti_bloat.run",
                json!({"review_id":initial_skip_id}),
            )
            .await;
            assert_eq!(no_send["state"], initial_skip["state"]);
            let skipped = call(
                &pool,
                &mut client,
                "command",
                "scope.anti_bloat.run",
                json!({"review_id":later_skip_id}),
            )
            .await;
            assert_eq!(
                skipped["state"],
                json!({"status":"no_call","reason":"session_skip"})
            );
            let skipped_replay = call(
                &pool,
                &mut client,
                "command",
                "scope.anti_bloat.run",
                json!({"review_id":later_skip_id}),
            )
            .await;
            assert_eq!(skipped_replay, skipped);
            let skipped_row: (String, Option<Uuid>, String, String) = sqlx::query_as(
                "SELECT state,origin_session_id,session_preference,request_preference \
                 FROM scope_anti_bloat_reviews WHERE review_id=$1",
            )
            .bind(later_skip_id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(skipped_row.0, "session_skipped");
            assert!(skipped_row.1.is_some());
            assert_eq!(
                (&*skipped_row.2, &*skipped_row.3),
                ("use_workspace", "use_workspace")
            );
            let reservations: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM scope_anti_bloat_budget_reservations WHERE review_id=$1",
            )
            .bind(later_skip_id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(reservations, 0);
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(150), listener.accept())
                    .await
                    .is_err(),
                "session skip before send must not POST"
            );
            call(
                &pool,
                &mut client,
                "command",
                "session.advisory.preference.set",
                json!({"expected_revision":1,"preference":"use_workspace"}),
            )
            .await;
        }
        let dispatch = enabled && expected != "no_call";
        let (http, quiet) = if dispatch {
            (
                Some(tokio::spawn(process::response(
                    listener,
                    pool.clone(),
                    review,
                    status,
                    abstain,
                    duplicate,
                    partial,
                ))),
                None,
            )
        } else {
            (None, Some(listener))
        };
        let result = call(
            &pool,
            &mut client,
            "command",
            "scope.anti_bloat.run",
            json!({"review_id":review}),
        )
        .await;
        assert_eq!(result["state"]["status"], expected, "{result}");
        if enabled && status == 200 && !abstain && !duplicate && partial == 0 {
            call(
                &pool,
                &mut client,
                "command",
                "session.advisory.preference.set",
                json!({"expected_revision":2,"preference":"skip"}),
            )
            .await;
        }
        if !enabled {
            assert_eq!(result["state"]["reason"], "provider_unconfigured");
        }
        if enabled && !dispatch {
            assert_eq!(result["state"]["reason"], "preflight_invalid_configuration");
        }
        let replay = call(
            &pool,
            &mut client,
            "command",
            "scope.anti_bloat.run",
            json!({"review_id":review}),
        )
        .await;
        assert_eq!(result, replay);
        if let Some(listener) = quiet {
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(150), listener.accept())
                    .await
                    .is_err(),
                "no-call must not POST"
            );
        }
        let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM scope_anti_bloat_budget_reservations WHERE review_id=$1),(SELECT count(*) FROM scope_anti_bloat_budget_consumptions WHERE review_id=$1)").bind(review).fetch_one(&pool).await.unwrap();
        assert_eq!(counts, if dispatch { (1, 1) } else { (0, 0) });
        if let Some(http) = http {
            let (request, raw, listener) = http.await.unwrap();
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(150), listener.accept())
                    .await
                    .is_err(),
                "replay must not POST"
            );
            let row:SealedAudit=sqlx::query_as("SELECT request_bytes,request_sha256,raw_response,response_sha256,response_http_status,request_adapter_identity,response_original_input_tokens,response_original_output_tokens,response_original_elapsed_ms,response_complete,original_transport_context FROM scope_anti_bloat_reviews WHERE review_id=$1").bind(review).fetch_one(&pool).await.unwrap();
            assert_eq!(row.0, request);
            assert_eq!(row.1, format!("{:x}", Sha256::digest(&request)));
            assert_eq!(row.2, raw);
            assert_eq!(row.3, format!("{:x}", Sha256::digest(&raw)));
            assert_eq!(row.4, Some(status.into()));
            assert_eq!(row.5, "tect.anti-bloat-typesafe-choice/1");
            assert_eq!((row.6, row.7), (None, None));
            assert!(row.8.is_some_and(|v| v >= 0));
            assert_eq!(row.9, Some(partial == 0));
            let context = row.10.unwrap();
            let failure = match partial {
                1 => Some("response-oversize"),
                2 => Some("response-body-read"),
                _ if status == 500 => Some("http-status"),
                _ => None,
            };
            assert_eq!(context["send_certainty"], "sent");
            assert_eq!(
                context["outcome"],
                if failure.is_some() {
                    "provider_failure"
                } else {
                    "provider_response"
                }
            );
            assert_eq!(context["provider_failure_code"], json!(failure));
            assert_eq!(
                context["raw_response_ref"],
                format!("sha256:{:x}", Sha256::digest(&raw))
            );
            if partial > 0 {
                let prefix: Value = serde_json::from_slice(&raw).unwrap();
                assert_eq!(prefix["usage"], json!({"input_tokens":7,"output_tokens":3}));
                if partial == 1 {
                    assert_eq!(raw.len(), 64 * 1024);
                }
            }
            let usage:(Option<i64>,Option<i64>,bool,bool)=sqlx::query_as("SELECT input_tokens,output_tokens,unknown_usage,exhausted_after_response FROM scope_anti_bloat_budget_consumptions WHERE review_id=$1").bind(review).fetch_one(&pool).await.unwrap();
            assert_eq!(
                usage,
                if duplicate || partial > 0 {
                    (None, None, true, true)
                } else if status == 500 {
                    (Some(20), Some(30), false, false)
                } else {
                    (Some(7), Some(3), false, false)
                }
            );
            audit::immutable(
                &pool,
                &runtime,
                &enrollment.auth,
                enrollment.tenant_id,
                review,
            )
            .await;
        } else {
            let metadata:(Option<bool>,Option<Value>,Option<Vec<u8>>)=sqlx::query_as("SELECT response_complete,original_transport_context,raw_response FROM scope_anti_bloat_reviews WHERE review_id=$1").bind(review).fetch_one(&pool).await.unwrap();
            assert_eq!(metadata, (None, None, None));
        }
        let scope_fixture: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1 AND provider='fixture'",
        )
        .bind(workspace)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            scope_fixture, 1,
            "admin-only selected-binding fixture, not Scope HTTP proof"
        );
        if expected == "ranked" {
            // Synthetic loopback response exercises the same-session caller effect; it is
            // not evidence that the real JEV provider selected this finding.
            let top = result["state"]["ranked_ids"][0].as_str().unwrap();
            let finding = prepared["findings"]
                .as_array()
                .unwrap()
                .iter()
                .find(|finding| finding["id"] == top && finding["rankable"] == true)
                .unwrap();
            let before: Value = sqlx::query_scalar(
                "SELECT payload FROM scope_candidate_drafts WHERE candidate_set_id=$1 AND set_revision=$2",
            )
            .bind(candidate)
            .bind(revision)
            .fetch_one(&pool)
            .await
            .unwrap();
            let removed = before["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["id"] == finding["candidate_id"])
                .unwrap();
            let applied = call(
                &pool,
                &mut client,
                "command",
                "scope.anti_bloat.apply",
                json!({"review_id":review,"finding_id":top,"disposition":"narrow",
                    "delta":{"candidate_set_id":candidate,"expected_revision":revision,
                        "idempotency_key":format!("synthetic-s04-{review}"),"operations":[
                            {"operation":"candidate.remove","candidate_id":finding["candidate_id"],
                             "expected_revision":removed["revision"]}]}}),
            )
            .await;
            assert_eq!(applied["from_revision"], revision);
            assert_eq!(applied["to_revision"], revision + 1);
            let after: Value = sqlx::query_scalar(
                "SELECT payload FROM scope_candidate_drafts WHERE candidate_set_id=$1 AND set_revision=$2",
            )
            .bind(candidate)
            .bind(revision + 1)
            .fetch_one(&pool)
            .await
            .unwrap();
            for field in ["goals", "evidence", "blockers", "protected_changes"] {
                assert_eq!(after[field], before[field], "preserved {field}");
            }
            let before_candidates = before["candidates"].as_array().unwrap();
            let after_candidates = after["candidates"].as_array().unwrap();
            assert_eq!(after_candidates.len() + 1, before_candidates.len());
            assert!(
                !after_candidates
                    .iter()
                    .any(|item| item["id"] == finding["candidate_id"])
            );
            for item in before_candidates
                .iter()
                .filter(|item| item["id"] != finding["candidate_id"])
            {
                assert!(
                    after_candidates.contains(item),
                    "retained candidate changed"
                );
            }
            let verifier =
                admin::prepare_verifier_enrollment(&pool, enrollment.tenant_id, workspace)
                    .await
                    .unwrap()
                    .try_commit()
                    .await
                    .unwrap();
            assert_ne!(verifier.principal_id, actor);
            let verifier_path = root.join("verifier.json");
            host_file(&verifier_path, &verifier.auth);
            let mut verifier_mcp = Mcp::start(
                &native_socket,
                &verifier_path,
                &Uuid::new_v4().to_string(),
                &key,
            )
            .await;
            call(
                &pool,
                &mut verifier_mcp,
                "command",
                "workspace.open",
                json!({}),
            )
            .await;
            let preservation = call(
                &pool,
                &mut verifier_mcp,
                "query",
                "scope.anti_bloat.preservation.get",
                json!({"review_id":review}),
            )
            .await;
            assert_eq!(preservation["verdict"], "pass", "{preservation}");
            assert_eq!(
                preservation["material"]["after_saved"]["goals"],
                before["goals"]
            );
            let attestation = call(
                &pool,
                &mut verifier_mcp,
                "command",
                "scope.anti_bloat.preservation.verify",
                json!({"request_id":Uuid::new_v4(),"review_id":review,
                    "expected_evidence_digest":preservation["evidence_digest"]}),
            )
            .await;
            assert_eq!(attestation["verdict"], "pass", "{attestation}");
            assert_eq!(
                attestation["verifier_principal_id"],
                verifier.principal_id.to_string()
            );
            let persisted: (i64, i64) = sqlx::query_as(
                "SELECT (SELECT count(*) FROM scope_anti_bloat_caller_links WHERE review_id=$1), \
                 (SELECT count(*) FROM scope_anti_bloat_preservation_attestations WHERE review_id=$1)",
            )
            .bind(review)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(persisted, (1, 1));
            verifier_mcp.finish().await;
        }
        client.finish().await;
        process::stop(&mut daemon).await;
    }
}
