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
);

// This guard is intentionally tied to the explicitly owned disposable cluster.
async fn identity(pool: &PgPool) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let row:(String,i64,String,i64)=sqlx::query_as("SELECT current_database(),d.oid::bigint,(SELECT system_identifier::text FROM pg_control_system()),(SELECT max(version) FROM _sqlx_migrations) FROM pg_database d WHERE datname=current_database()")
        .fetch_one(pool).await.unwrap();
    assert_eq!(
        row,
        ("tect_test".into(), 16385, "7689676854994613066".into(), 99)
    );
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
#[ignore = "requires the explicitly owned disposable PostgreSQL 18 fixture"]
async fn native_anti_bloat_public_mcp_is_sealed_once_and_default_disabled() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let pool = PgPool::connect(&admin_url).await.unwrap();
    identity(&pool).await;
    for (enabled, status, abstain, expected) in [
        (true, 200, false, "ranked"),
        (true, 200, true, "provider_abstained"),
        (true, 500, false, "invalid_response"),
        (false, 200, false, "no_call"),
        (true, 200, false, "no_call"),
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
        let dispatch = enabled && expected != "no_call";
        let (http, quiet) = if dispatch {
            (
                Some(tokio::spawn(process::response(
                    listener,
                    pool.clone(),
                    review,
                    status,
                    abstain,
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
            let row:SealedAudit=sqlx::query_as("SELECT request_bytes,request_sha256,raw_response,response_sha256,response_http_status,request_adapter_identity,response_original_input_tokens,response_original_output_tokens,response_original_elapsed_ms FROM scope_anti_bloat_reviews WHERE review_id=$1").bind(review).fetch_one(&pool).await.unwrap();
            assert_eq!(row.0, request);
            assert_eq!(row.1, format!("{:x}", Sha256::digest(&request)));
            assert_eq!(row.2, raw);
            assert_eq!(row.3, format!("{:x}", Sha256::digest(&raw)));
            assert_eq!(row.4, Some(status.into()));
            assert_eq!(row.5, "tect.anti-bloat-typesafe-choice/1");
            assert_eq!((row.6, row.7), (None, None));
            assert!(row.8.is_some_and(|v| v >= 0));
            let usage:(Option<i64>,Option<i64>,bool,bool)=sqlx::query_as("SELECT input_tokens,output_tokens,unknown_usage,exhausted_after_response FROM scope_anti_bloat_budget_consumptions WHERE review_id=$1").bind(review).fetch_one(&pool).await.unwrap();
            assert_eq!(usage, (Some(7), Some(3), false, false));
            audit::immutable(
                &pool,
                &runtime,
                &enrollment.auth,
                enrollment.tenant_id,
                review,
            )
            .await;
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
        client.finish().await;
        process::stop(&mut daemon).await;
    }
}
