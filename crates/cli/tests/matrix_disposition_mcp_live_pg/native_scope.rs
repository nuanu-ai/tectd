//! Native Scope's four-level wire, sealed receipt and public MCP boundary.
use super::*;
use std::time::Duration;
use tect_application::{ScopeBudgetPolicy, ScopeBudgetPolicyEvaluation, ScopeBudgetRequest};
use tect_host::{JevScopeAdviceConfig, JevScopeAdviceProvider};
use tect_postgres::{BudgetOwnerKeys, PgScopeAuthoredManifestSupplier, PgScopeAuthorityObserver};
use tokio::net::TcpListener;
#[path = "native_scope/source.rs"]
mod source;
#[path = "native_scope/transport.rs"]
mod transport;

#[derive(Clone, Copy, Debug)]
enum Case {
    Preferred,
    NoPreference,
    Http500,
    Malformed,
    InvalidAnswers,
    DuplicateUsage,
    Partial,
    Revoked,
}
struct SignedFixtureBudget;
#[async_trait]
impl ScopeBudgetPolicy for SignedFixtureBudget {
    async fn evaluate(
        &self,
        _: &ScopeBudgetRequest,
        policy: &tect_domain::AdvisoryBudgetPolicy,
    ) -> Result<Option<ScopeBudgetPolicyEvaluation>> {
        Ok(Some(ScopeBudgetPolicyEvaluation {
            policy_id: policy.id().to_string(),
            policy_version: policy.version(),
            policy_digest: policy.digest().into(),
        }))
    }
}
async fn call(pool: &PgPool, client: &mut Mcp, name: &str, params: Value) -> Value {
    decomposition_parent::guard(pool).await;
    route(client, "command", name, params).await
}
async fn no_http(listener: &TcpListener) {
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
}
async fn exercise(case: Case) {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    for (url, user) in [(&admin_url, "postgres"), (&runtime_url, "tect_ci")] {
        let options = PgConnectOptions::from_str(url).unwrap();
        assert_eq!(options.get_host(), "127.0.0.1");
        assert_eq!(options.get_port(), 64775);
        assert_eq!(options.get_database(), Some("tect_test"));
        assert_eq!(options.get_username(), user);
    }
    let pool = PgPool::connect(&admin_url).await.unwrap();
    decomposition_parent::guard(&pool).await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    support::repository(&repo);
    let socket = root.join("bootstrap.sock");
    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
    let bootstrap = Arc::new(WorkspaceService::new(
        Arc::new(store.clone()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    ));
    let unix = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(unix, bootstrap));
    decomposition_parent::guard(&pool).await;
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("owner.json");
    host_file(&config, &enrolled.auth);
    let native = Uuid::new_v4().to_string();
    let key = Uuid::new_v4().to_string();
    let mut owner = Mcp::start(&socket, &config, &native, &key).await;
    let opened = call(&pool, &mut owner, "workspace.open", json!({})).await;
    let workspace = Uuid::parse_str(opened["workspace"]["id"].as_str().unwrap()).unwrap();
    let request = source::authored(&pool, &mut owner, &repo).await;
    let set = Uuid::parse_str(request["candidate_set_id"].as_str().unwrap()).unwrap();
    let baseline = source::counts(&pool, workspace, set).await;
    let keys = model_route_native::setup::budget(
        &pool,
        &store,
        &enrolled.auth,
        enrolled.tenant_id,
        workspace,
    )
    .await;
    owner.finish().await;
    server.abort();
    let _ = server.await;
    let http = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/systemone", http.local_addr().unwrap());
    let trusted =
        store.with_budget_owner_keys(BudgetOwnerKeys::from_json(&keys.to_string()).unwrap());
    let authority = Arc::new(PgScopeAuthorityObserver::new(
        trusted.clone(),
        Arc::new(tect_host::StaticCandidateGuidance),
    ));
    let supplier = Arc::new(PgScopeAuthoredManifestSupplier::new(
        trusted.clone(),
        authority.clone(),
    ));
    let provider = JevScopeAdviceProvider::new(
        JevScopeAdviceConfig {
            profile: "native-scope-fixture".into(),
            endpoint: endpoint.parse().unwrap(),
            model: "jev-1.13.0".into(),
            timeout: Duration::from_secs(3),
            maximum_request_bytes: 512 * 1024,
            maximum_response_bytes: 64 * 1024,
        },
        "synthetic-fixture-only".into(),
    )
    .unwrap();
    let service = Arc::new(WorkspaceService::new_with_scope_advisory_adapters(
        Arc::new(trusted),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
        authority,
        supplier,
        Arc::new(SignedFixtureBudget),
        Arc::new(provider),
    ));
    let socket = root.join("native.sock");
    let unix = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(unix, service));
    let mut owner = Mcp::start(&socket, &config, &native, &key).await;
    call(&pool, &mut owner, "workspace.open", json!({})).await;
    call(&pool,&mut owner,"workspace.advisory.configure",json!({"expected_revision":0,"mode":"optional",
        "provider_profile_ref":{"id":"native-scope-fixture"},"model_configuration":{"model":"jev-1.13.0"}})).await;
    let stub = tokio::spawn(transport::serve_once(
        http,
        pool.clone(),
        workspace,
        enrolled.auth.host_id,
        native,
        case,
    ));
    decomposition_parent::guard(&pool).await;
    let result = if matches!(case, Case::Revoked) {
        route_error(
            &mut owner,
            "command",
            "scope.advisory.request",
            request.clone(),
        )
        .await
    } else {
        route(
            &mut owner,
            "command",
            "scope.advisory.request",
            request.clone(),
        )
        .await
    };
    let (http, sent, raw) = stub.await.unwrap();
    no_http(&http).await;
    audit(
        &pool,
        &runtime_url,
        enrolled.tenant_id,
        workspace,
        &sent,
        &raw,
        case,
    )
    .await;
    assert_eq!(source::counts(&pool, workspace, set).await, baseline);
    if matches!(case, Case::Revoked) {
        assert_error(&result, &["session_revoked"]);
    } else {
        let current = route(
            &mut owner,
            "query",
            "candidate.advisory.get",
            json!({
            "candidate_set_id":set,"opportunity_id":result["opportunity_id"]}),
        )
        .await;
        if matches!(case, Case::Preferred | Case::NoPreference) {
            assert_eq!(result["state"], "advised");
            let advice = &current["scope_decomposition"]["advice"];
            let items = advice["items"].as_array().unwrap();
            assert_eq!(items.len(), 2);
            assert_eq!(advice["ranked_ids"].as_array().unwrap().len(), 2);
            if matches!(case, Case::Preferred) {
                assert_eq!(items[0]["choice"], "preferred");
                assert_eq!(items[0]["score"], "strong_fit");
                assert_eq!(advice["ranked_ids"][0], items[0]["alternative_id"]);
                assert_eq!(items[1]["choice"], "non_preferred");
            } else {
                assert!(
                    items.iter().all(
                        |item| item["choice"] == "non_preferred" && item["score"] == "weak_fit"
                    )
                );
            }
        } else {
            assert!(current.get("scope_decomposition").is_none());
            if matches!(case, Case::InvalidAnswers) {
                assert_eq!(result["state"], "failed");
                assert_eq!(result["reason"], "provider_failure");
            }
        }
        let replay = call(&pool, &mut owner, "scope.advisory.request", request).await;
        assert_eq!(replay["opportunity_id"], result["opportunity_id"]);
        assert_eq!(replay["state"], result["state"]);
        assert_eq!(replay["advice_id"], result["advice_id"]);
        no_http(&http).await;
        audit(
            &pool,
            &runtime_url,
            enrolled.tenant_id,
            workspace,
            &sent,
            &raw,
            case,
        )
        .await;
        assert_eq!(source::counts(&pool, workspace, set).await, baseline);
    }
    println!(
        "native Scope {case:?}: one loopback POST, sealed original raw+usage, no caller effects"
    );
    owner.finish().await;
    server.abort();
    let _ = server.await;
}

type RawAudit = (
    Vec<u8>,
    String,
    Vec<u8>,
    String,
    i32,
    bool,
    String,
    Option<String>,
    Option<String>,
    i64,
    Value,
);
async fn audit(
    pool: &PgPool,
    url: &str,
    tenant: Uuid,
    workspace: Uuid,
    sent: &[u8],
    raw: &[u8],
    case: Case,
) {
    decomposition_parent::guard(pool).await;
    let runtime = PgPool::connect(url).await.unwrap();
    let mut tx = runtime.begin().await.unwrap();
    sqlx::query("SELECT set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let row:RawAudit=sqlx::query_as("SELECT d.request_payload,o.request_sha256,o.response_payload,o.response_sha256,o.http_status,o.response_complete,o.original_transport_outcome,o.original_input_tokens,o.original_output_tokens,o.elapsed_ms,o.original_transport_context FROM advisory_provider_observations o JOIN advisory_dispatch d ON d.id=o.dispatch_id AND d.workspace_id=o.workspace_id WHERE o.workspace_id=$1")
        .bind(workspace).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(row.0, sent);
    assert_eq!(row.1, format!("{:x}", Sha256::digest(sent)));
    assert_eq!(row.2, raw);
    assert_eq!(row.3, format!("{:x}", Sha256::digest(raw)));
    assert_eq!(
        row.4,
        if matches!(case, Case::Http500) {
            500
        } else {
            200
        }
    );
    let partial = matches!(case, Case::Partial);
    assert_eq!(row.5, !partial);
    assert_eq!(
        row.6,
        if partial {
            "partial_received"
        } else {
            "received"
        }
    );
    assert_eq!((row.7, row.8), (None, None));
    assert!(row.9 >= 0);
    assert_eq!(row.10["send_certainty"], "sent");
    assert_eq!(
        row.10["outcome"],
        if matches!(case, Case::Http500 | Case::Partial) {
            "provider_failure"
        } else {
            "provider_response"
        }
    );
    let failure = if matches!(case, Case::Http500) {
        Some("http-status")
    } else if partial {
        Some("oversize")
    } else {
        None
    };
    assert_eq!(row.10["provider_failure_code"].as_str(), failure);
    assert!(
        row.10["raw_response_ref"]
            .as_str()
            .unwrap()
            .contains(&row.3)
    );
    if partial {
        let prefix: Value = serde_json::from_slice(raw).unwrap();
        assert_eq!(
            prefix["usage"],
            json!({"input_tokens":20,"output_tokens":30})
        );
        assert_eq!(raw.len(), 64 * 1024);
    }
    let counts:(i64,i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM advisory_provider_observations WHERE workspace_id=$1),(SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1),(SELECT count(*) FROM advisory_budget_reservations WHERE workspace_id=$1),(SELECT count(*) FROM advisory_budget_consumptions WHERE workspace_id=$1)")
        .bind(workspace).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(counts, (1, 1, 1, 1));
    let usage:(Option<i64>,Option<i64>,bool)=sqlx::query_as("SELECT c.input_tokens,c.output_tokens,c.unknown_usage FROM advisory_budget_consumptions c JOIN advisory_dispatch d ON d.id=c.dispatch_id AND d.workspace_id=c.workspace_id WHERE c.workspace_id=$1 AND c.input_tokens IS NOT DISTINCT FROM d.input_tokens AND c.output_tokens IS NOT DISTINCT FROM d.output_tokens")
        .bind(workspace).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(
        usage,
        if matches!(case, Case::Partial | Case::Malformed | Case::DuplicateUsage) {
            (None, None, true)
        } else {
            (Some(20), Some(30), false)
        }
    );
    let effects:(i64,i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM advisory_scope_disposition WHERE workspace_id=$1),(SELECT count(*) FROM advisory_scope_preservation_receipt WHERE workspace_id=$1),(SELECT count(*) FROM advisory_scope_caller_link WHERE workspace_id=$1),(SELECT count(*) FROM native_scopes WHERE workspace_id=$1)")
        .bind(workspace).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(effects, (0, 0, 0, 0));
    let advice: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_scope_advice WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(
        advice,
        if matches!(case, Case::Preferred | Case::NoPreference) {
            1
        } else {
            0
        }
    );
    tx.commit().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicitly owned disposable PG18.6 migration100; no real JEV"]
async fn public_native_scope_preserves_receipts_and_never_selects_caller_effects() {
    for case in [
        Case::Preferred,
        Case::NoPreference,
        Case::Http500,
        Case::Malformed,
        Case::InvalidAnswers,
        Case::DuplicateUsage,
        Case::Partial,
        Case::Revoked,
    ] {
        exercise(case).await;
    }
}
