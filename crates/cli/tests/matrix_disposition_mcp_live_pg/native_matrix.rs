//! Actual native HTTP adapter through public MCP; only a synthetic loopback stub.
use super::*;
use std::time::Duration;
use tect_host::jev_matrix_advice::native_provider::{
    JevNativeMatrixConfig, JevNativeMatrixProvider,
};
use tect_postgres::BudgetOwnerKeys;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[derive(Clone, Copy, Debug)]
enum Case {
    Ranked,
    Abstained,
    Http500,
    Malformed,
    Revoked,
}

type RawAudit = (
    Vec<u8>,
    String,
    Vec<u8>,
    String,
    i32,
    Option<String>,
    Option<String>,
    i64,
    String,
    bool,
    String,
    String,
);

async fn guarded(pool: &PgPool, client: &mut Mcp, name: &str, params: Value) -> Value {
    decomposition_parent::guard(pool).await;
    route(client, "command", name, params).await
}

fn response(request: &Value, abstain: bool) -> Vec<u8> {
    let mut answers = serde_json::Map::new();
    for (token, level) in [("C0", 4), ("C1", 8)] {
        let question = format!("score_v1_{token}");
        let legend = request["questions"][&question]["criteria"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, value)| (i.to_string(), value.clone()))
            .collect::<serde_json::Map<_, _>>();
        let probabilities = (0..10)
            .map(|i| (i.to_string(), json!(if i == level { 1.0 } else { 0.0 })))
            .collect::<serde_json::Map<_, _>>();
        answers.insert(
            question,
            json!({"type":"score","score":level,"legend":legend,
            "probabilities":probabilities,"confidence":0.9}),
        );
    }
    answers.insert(
        "choice_v1".into(),
        json!({"type":"choice",
        "choice":if abstain {"ABSTAIN"} else {"C1"},
        "probabilities":if abstain {json!({"C0":0.05,"C1":0.05,"ABSTAIN":0.9})}
            else {json!({"C0":0.1,"C1":0.8,"ABSTAIN":0.1})},"confidence":0.9}),
    );
    serde_json::to_vec(&json!({"model":request["model"],"answers":answers,
        "usage":{"input_tokens":20,"output_tokens":30}}))
    .unwrap()
}

async fn serve_once(
    listener: TcpListener,
    pool: PgPool,
    workspace: Uuid,
    host: Uuid,
    native: String,
    case: Case,
) -> (TcpListener, Vec<u8>, Vec<u8>) {
    let (mut connection, _) = listener.accept().await.unwrap();
    let mut bytes = Vec::new();
    let head_end;
    let length;
    loop {
        let mut chunk = [0u8; 4096];
        let read = connection.read(&mut chunk).await.unwrap();
        assert!(read > 0);
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(index) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
            head_end = index + 4;
            let headers = std::str::from_utf8(&bytes[..head_end]).unwrap();
            assert!(headers.starts_with("POST /v1/systemone HTTP/1.1\r\n"));
            assert!(headers.lines().any(|line| {
                line.split_once(':').is_some_and(|(key, value)| {
                    key.eq_ignore_ascii_case("authorization")
                        && value.trim() == "Bearer synthetic-fixture-only"
                })
            }));
            length = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                        .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            break;
        }
    }
    while bytes.len() < head_end + length {
        let mut chunk = [0u8; 4096];
        let read = connection.read(&mut chunk).await.unwrap();
        assert!(read > 0);
        bytes.extend_from_slice(&chunk[..read]);
    }
    let body = bytes[head_end..head_end + length].to_vec();
    let sent: (String, String, i64) = sqlx::query_as("SELECT state,send_certainty,(SELECT count(*) FROM advisory_budget_reservations WHERE workspace_id=$1) FROM advisory_dispatch WHERE workspace_id=$1")
        .bind(workspace).fetch_one(&pool).await.unwrap();
    assert_eq!(sent, ("sending".into(), "sent_unknown".into(), 1));
    if matches!(case, Case::Revoked) {
        // Explicitly authorized disposable-fixture ACL change. No immutable
        // Matrix source, lineage or raw-evidence row is modified.
        decomposition_parent::guard(&pool).await;
        let changed = sqlx::query("UPDATE agent_sessions SET revoked=true WHERE host_id=$1 AND native_session_id=$2 AND workspace_id=$3 AND NOT revoked")
            .bind(host).bind(native).bind(workspace).execute(&pool).await.unwrap();
        assert_eq!(changed.rows_affected(), 1);
    }
    let raw = if matches!(case, Case::Malformed) {
        b"{".to_vec()
    } else {
        response(
            &serde_json::from_slice(&body).unwrap(),
            matches!(case, Case::Abstained),
        )
    };
    let status = if matches!(case, Case::Http500) {
        "500 Internal Server Error"
    } else {
        "200 OK"
    };
    connection.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", raw.len()).as_bytes()).await.unwrap();
    connection.write_all(&raw).await.unwrap();
    (listener, body, raw)
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
    let socket = root.join("matrix.sock");
    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
    let disabled = Arc::new(WorkspaceService::new(
        Arc::new(store.clone()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    ));
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let bootstrap = tokio::spawn(tect_host::serve(listener, disabled));
    decomposition_parent::guard(&pool).await;
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("owner.json");
    host_file(&config, &enrolled.auth);
    let native = Uuid::new_v4().to_string();
    let key = Uuid::new_v4().to_string();
    let mut owner = Mcp::start(&socket, &config, &native, &key).await;
    let opened = guarded(&pool, &mut owner, "workspace.open", json!({})).await;
    let workspace = Uuid::parse_str(opened["workspace"]["id"].as_str().unwrap()).unwrap();
    let keys = model_route_native::setup::budget(
        &pool,
        &store,
        &enrolled.auth,
        enrolled.tenant_id,
        workspace,
    )
    .await;
    owner.finish().await;
    bootstrap.abort();
    let _ = bootstrap.await;
    // UnixListener has no unlink-on-drop; remove only this private fixture socket.
    std::fs::remove_file(&socket).unwrap();
    let http = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/systemone", http.local_addr().unwrap());
    let profile = "native-matrix-fixture";
    let model = "jev-1.13.0";
    let provider = JevNativeMatrixProvider::new(
        JevNativeMatrixConfig {
            provider_identity: MatrixProviderIdentity {
                provider_profile_ref: AdvisoryProviderProfileRef { id: profile.into() },
                model_configuration: AdvisoryModelConfiguration {
                    model: model.into(),
                },
                destination: endpoint.clone(),
                wire_version: "tect.matrix-typesafe-native/1".into(),
            },
            endpoint: endpoint.parse().unwrap(),
            timeout: Duration::from_secs(3),
            maximum_request_bytes: 512 * 1024,
            maximum_response_bytes: 64 * 1024,
        },
        "synthetic-fixture-only".into(),
    )
    .unwrap();
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(
                store
                    .with_budget_owner_keys(BudgetOwnerKeys::from_json(&keys.to_string()).unwrap()),
            ),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(Evidence(Arc::new(AtomicBool::new(false)))))
        .with_matrix_advisory_adapters(Arc::new(provider), Arc::new(Budget)),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    let mut owner = Mcp::start(&socket, &config, &native, &key).await;
    guarded(&pool, &mut owner, "workspace.open", json!({})).await;
    guarded(&pool,&mut owner,"workspace.advisory.configure",json!({"expected_revision":0,"mode":"optional","provider_profile_ref":{"id":profile},"model_configuration":{"model":model}})).await;
    decomposition_parent::guard(&pool).await;
    let verifier = admin::prepare_verifier_enrollment(&pool, enrolled.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let verifier_config = root.join("verifier.json");
    host_file(&verifier_config, &verifier.auth);
    let mut independent =
        Mcp::start(&socket, &verifier_config, &Uuid::new_v4().to_string(), &key).await;
    guarded(&pool, &mut independent, "workspace.open", json!({})).await;
    let task = Uuid::new_v4();
    decomposition_parent::guard(&pool).await;
    let recorded = record_task(&mut owner, task, &["a", "b"]).await;
    decomposition_parent::guard(&pool).await;
    verify(&mut independent, &recorded, task).await;
    let request_key = format!("native-{task}");
    let request = json!({"task_id":task,"expected_task_revision":1,"request_key":request_key});
    let stub = tokio::spawn(serve_once(
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
            "engineering.advisory.request",
            request.clone(),
        )
        .await
    } else {
        route(
            &mut owner,
            "command",
            "engineering.advisory.request",
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
    if matches!(case, Case::Revoked) {
        assert_error(&result, &["session_revoked"]);
    } else {
        let current = route(
            &mut owner,
            "query",
            "engineering.advisory.get",
            json!({"task_id":task,"request_key":request_key}),
        )
        .await;
        match case {
            Case::Ranked => {
                assert_eq!(result["state"], "advised");
                assert_eq!(
                    current["current_advice"]["outcome"]["ranked_choice_ids"],
                    json!(["b", "a"])
                );
            }
            Case::Abstained => {
                assert_eq!(result["state"], "advised");
                assert_eq!(current["current_advice"]["outcome"]["status"], "abstained");
            }
            _ => assert!(current.get("current_advice").is_none()),
        }
        let replay = guarded(&pool, &mut owner, "engineering.advisory.request", request).await;
        assert_eq!(replay["opportunity_id"], result["opportunity_id"]);
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
    }
    println!(
        "native Matrix {case:?}: one loopback POST, exact durable raw/status/hash/usage, no automatic effects"
    );
    independent.finish().await;
    owner.finish().await;
    server.abort();
    let _ = server.await;
}

async fn audit(
    pool: &PgPool,
    runtime: &str,
    tenant: Uuid,
    workspace: Uuid,
    sent: &[u8],
    raw: &[u8],
    case: Case,
) {
    decomposition_parent::guard(pool).await;
    let runtime = PgPool::connect(runtime).await.unwrap();
    let mut tx = runtime.begin().await.unwrap();
    let role: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(role, "tect_ci");
    sqlx::query("SELECT set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let row:RawAudit=sqlx::query_as(
        "SELECT d.request_payload,o.request_sha256,o.response_payload,o.response_sha256,o.http_status,o.original_input_tokens,o.original_output_tokens,o.elapsed_ms,o.original_transport_outcome,o.response_complete,d.configuration_digest,o.configuration_digest FROM advisory_dispatch d JOIN advisory_provider_observations o ON o.dispatch_id=d.id AND o.workspace_id=d.workspace_id WHERE d.workspace_id=$1")
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
    assert_eq!((row.5, row.6), (None, None));
    assert!(row.7 >= 0);
    assert_eq!(row.8, "received");
    assert!(row.9);
    assert_eq!(row.10, row.11);
    let identity: (String, String, Value, String) = sqlx::query_as(
        "SELECT provider,model,configuration_snapshot,configuration_digest \
         FROM advisory_dispatch WHERE workspace_id=$1",
    )
    .bind(workspace)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(identity.0, "native-matrix-fixture");
    assert_eq!(identity.1, "jev-1.13.0");
    assert_eq!(identity.2["provider_profile_ref"]["id"], identity.0);
    assert_eq!(identity.2["model_configuration"]["model"], identity.1);
    assert_eq!(identity.2["wire_version"], "tect.matrix-typesafe-native/1");
    assert!(
        identity.2["destination"]
            .as_str()
            .unwrap()
            .starts_with("http://127.0.0.1:")
    );
    assert!(
        identity.2["destination"]
            .as_str()
            .unwrap()
            .ends_with("/v1/systemone")
    );
    assert_eq!(identity.2["request_body_sha256"], row.1);
    assert_eq!(identity.2["request_body_length"], sent.len());
    assert_eq!(
        identity.3,
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&identity.2).unwrap())
        )
    );
    assert_eq!(
        identity.2["budget_policy_id"],
        identity.2["budget_policy"]["policy_id"]
    );
    let accounting:(i64,i64,i64,Option<i64>,Option<i64>,bool)=sqlx::query_as(
        "SELECT (SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1),(SELECT count(*) FROM advisory_budget_reservations WHERE workspace_id=$1),(SELECT count(*) FROM advisory_budget_consumptions WHERE workspace_id=$1),input_tokens,output_tokens,unknown_usage FROM advisory_budget_consumptions WHERE workspace_id=$1")
        .bind(workspace).fetch_one(&mut *tx).await.unwrap();
    assert_eq!((accounting.0, accounting.1, accounting.2), (1, 1, 1));
    if matches!(case, Case::Malformed) {
        assert_eq!(
            (accounting.3, accounting.4, accounting.5),
            (None, None, true)
        );
    } else {
        assert_eq!(
            (accounting.3, accounting.4, accounting.5),
            (Some(20), Some(30), false)
        );
    }
    let effects:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM advisory_matrix_disposition WHERE workspace_id=$1),(SELECT count(*) FROM scope_candidate_sets WHERE workspace_id=$1),(SELECT count(*) FROM matrix_planning_selection_links WHERE workspace_id=$1)")
        .bind(workspace).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(effects, (0, 0, 0));
    let advice: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_matrix_advice WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(
        advice,
        if matches!(case, Case::Ranked | Case::Abstained) {
            1
        } else {
            0
        }
    );
    tx.commit().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicitly owned disposable PG18.6 migration98; no real JEV"]
async fn public_native_matrix_retains_raw_before_ranking_and_after_revocation() {
    for case in [
        Case::Ranked,
        Case::Abstained,
        Case::Http500,
        Case::Malformed,
        Case::Revoked,
    ] {
        exercise(case).await;
    }
}
