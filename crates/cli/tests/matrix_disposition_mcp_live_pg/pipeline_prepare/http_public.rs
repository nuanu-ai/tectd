use super::*;
use std::time::Duration;
use tect_host::jev_pipeline_recommendation::{
    JevPipelineConfig, JevPipelineProvider, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, WIRE_VERSION,
    prepare_native_request,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use url::Url;

pub(super) async fn exercise(fixture: &NoCallFixture<'_>, independent: &mut Mcp) {
    exercise_inner(fixture, Some(independent), None, None).await;
}

pub(super) async fn exercise_vertical(fixture: &NoCallFixture<'_>, ready: &Value) {
    exercise_inner(fixture, None, Some(ready), None).await;
}

pub(super) async fn exercise_case(fixture: &NoCallFixture<'_>, case: super::native_cases::Case) {
    exercise_inner(fixture, None, None, Some(case)).await;
}

async fn exercise_inner(
    fixture: &NoCallFixture<'_>,
    independent: Option<&mut Mcp>,
    ready: Option<&Value>,
    case: Option<super::native_cases::Case>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = Url::parse(&format!(
        "http://{}/v1/systemone",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let socket = fixture.root.join("pipeline-http-public.sock");
    let provider = JevPipelineProvider::new(
        JevPipelineConfig {
            identity: PipelineProviderIdentity {
                provider: "fixture-systemone".into(),
                model: "jev-1.13.0".into(),
                destination: endpoint.as_str().into(),
                wire_version: WIRE_VERSION.into(),
            },
            endpoint,
            timeout: Duration::from_secs(2),
            maximum_request_bytes: MAX_REQUEST_BYTES,
            maximum_response_bytes: MAX_RESPONSE_BYTES,
        },
        "fixture-secret".into(),
    )
    .unwrap();
    let store = PgStore::connect(fixture.runtime_url, 4)
        .await
        .unwrap()
        .with_budget_owner_keys(fixture.budget_owner_keys.clone());
    let store: Arc<dyn Store> = if ready.is_some() {
        Arc::new(super::native_recovery::InterruptAfterRaw {
            inner: store,
            pending: AtomicBool::new(true),
        })
    } else {
        Arc::new(store)
    };
    let service = Arc::new(
        WorkspaceService::new(
            store,
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_pipeline_recommendation_definitions(Arc::new(
            tect_host::StaticPipelineRecommendationDefinitions,
        ))
        .with_pipeline_compatibility_policy(Arc::new(FixedPipelineCompatibilityPolicy(
            explicit_fixture_policy(fixture.task),
        )))
        .with_pipeline_recommendation_provider(Arc::new(provider)),
    );
    let unix = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(unix, service));
    let caller_native = Uuid::new_v4().to_string();
    let mut owner = Mcp::start(
        &socket,
        fixture.owner_config,
        &caller_native,
        fixture.workspace_key,
    )
    .await;
    owner.call("open_workspace", json!({})).await;
    route(
        &mut owner,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":1,"mode":"optional",
            "provider_profile_ref":{"id":"fixture-systemone"},
            "model_configuration":{"model":"jev-1.13.0"}
        }),
    )
    .await;

    if let Some(case) = case {
        let listener = super::native_cases::exercise(fixture, &socket, listener, case).await;
        assert_no_http(&listener).await;
        owner.finish().await;
        server.abort();
        return;
    }
    let revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM slice_candidate_sets WHERE workspace_id=$1 AND id=$2",
    )
    .bind(fixture.workspace)
    .bind(fixture.set)
    .fetch_one(fixture.pool)
    .await
    .unwrap();
    let prepared = route(
        &mut owner,
        "command",
        "pipeline.recommendation.prepare",
        json!({
            "candidate_set_id":fixture.set,
            "expected_candidate_set_revision":revision,
            "work_node_id":fixture.work["id"],
            "expected_work_node_revision":fixture.work["revision"],
            "request_key":format!("pipeline-http-{}", Uuid::new_v4())
        }),
    )
    .await;
    assert_eq!(prepared["state"], "prepared", "{prepared}");
    let opportunity = Uuid::parse_str(prepared["opportunity_id"].as_str().unwrap()).unwrap();
    let manifest_value: Value = sqlx::query_scalar(
        "SELECT manifest_payload FROM pipeline_advice_contexts WHERE workspace_id=$1 AND opportunity_id=$2",
    )
    .bind(fixture.workspace)
    .bind(opportunity)
    .fetch_one(fixture.pool)
    .await
    .unwrap();
    let manifest: PipelineRecommendationManifest = serde_json::from_value(manifest_value).unwrap();
    let frozen = prepare_native_request("jev-1.13.0", &manifest, MAX_REQUEST_BYTES).unwrap();
    let expected_hash = format!("{:x}", Sha256::digest(&frozen.body));
    let raw_response = native_response(&frozen.body, &frozen.eligible_ids);
    let run = json!({"opportunity_id":opportunity});

    if let Some(independent) = independent {
        assert_error(
            &route_error(
                independent,
                "command",
                "pipeline.recommendation.run",
                run.clone(),
            )
            .await,
            &["forbidden"],
        );
    }
    assert_error(
        &route_error(
            &mut owner,
            "command",
            "pipeline.recommendation.run",
            json!({"opportunity_id":Uuid::new_v4()}),
        )
        .await,
        &["not_found"],
    );
    assert_no_http(&listener).await;
    let dispatches: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1 AND opportunity_id=$2",
    )
    .bind(fixture.workspace)
    .bind(opportunity)
    .fetch_one(fixture.pool)
    .await
    .unwrap();
    assert_eq!(dispatches, 0);

    let response_for_stub = raw_response.clone();
    let audit_pool = fixture.pool.clone();
    let workspace = fixture.workspace;
    let stub = tokio::spawn(async move {
        let (mut connection, _) = listener.accept().await.unwrap();
        // The request is observable only after the sending row was committed.
        let committed: (String, bool, i64) = sqlx::query_as(
            "SELECT state,send_started_at IS NOT NULL,count(*) OVER () \
             FROM advisory_dispatch WHERE workspace_id=$1 AND opportunity_id=$2",
        )
        .bind(workspace)
        .bind(opportunity)
        .fetch_one(&audit_pool)
        .await
        .unwrap();
        assert_eq!(committed, ("sending".into(), true, 1));
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let head_end = loop {
            let read = connection.read(&mut buffer).await.unwrap();
            assert!(read > 0, "HTTP request ended before headers");
            request.extend_from_slice(&buffer[..read]);
            if let Some(index) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..head_end]);
        let length: usize = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().unwrap())
            })
            .unwrap();
        while request.len() < head_end + length {
            let read = connection.read(&mut buffer).await.unwrap();
            assert!(read > 0, "HTTP request ended before body");
            request.extend_from_slice(&buffer[..read]);
        }
        let response_headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response_for_stub.len()
        );
        connection
            .write_all(response_headers.as_bytes())
            .await
            .unwrap();
        connection.write_all(&response_for_stub).await.unwrap();
        (listener, request, head_end)
    });

    if ready.is_some() {
        let interrupted = route_error(
            &mut owner,
            "command",
            "pipeline.recommendation.run",
            run.clone(),
        )
        .await;
        assert_error(&interrupted, &["storage_unavailable"]);
        let pending:(String,i64,i64,i64)=sqlx::query_as("SELECT state,(SELECT count(*) FROM advisory_provider_observations WHERE dispatch_id=d.id),(SELECT count(*) FROM advisory_budget_reservations WHERE dispatch_id=d.id),(SELECT count(*) FROM advisory_budget_consumptions WHERE dispatch_id=d.id) FROM advisory_dispatch d WHERE opportunity_id=$1").bind(opportunity).fetch_one(fixture.pool).await.unwrap();
        assert_eq!(pending, ("sending".into(), 1, 1, 0));
        owner.finish().await;
        owner = Mcp::start(
            &socket,
            fixture.owner_config,
            &Uuid::new_v4().to_string(),
            fixture.workspace_key,
        )
        .await;
        owner.call("open_workspace", json!({})).await;
    }
    let ranked = route(
        &mut owner,
        "command",
        "pipeline.recommendation.run",
        run.clone(),
    )
    .await;
    assert_eq!(ranked["status"], "ranked", "{ranked}");
    assert_eq!(ranked["ranked_ids"], prepared["eligible_option_ids"]);
    let (listener, request, head_end) = stub.await.unwrap();
    let headers = String::from_utf8_lossy(&request[..head_end]);
    assert!(headers.starts_with("POST /v1/systemone HTTP/1.1\r\n"));
    assert!(headers.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("authorization") && value.trim() == "Bearer fixture-secret"
        })
    }));
    assert_eq!(&request[head_end..], frozen.body);

    let dispatch_id = Uuid::parse_str(ranked["dispatch_id"].as_str().unwrap()).unwrap();
    let audit: (
        String,
        String,
        String,
        String,
        String,
        Vec<u8>,
        Vec<u8>,
        String,
    ) = sqlx::query_as(
        "SELECT state,send_certainty,outcome,material_digest,payload_digest,request_payload,\
             response_payload,pipeline_response_sha256 \
             FROM advisory_dispatch WHERE workspace_id=$1 AND id=$2",
    )
    .bind(fixture.workspace)
    .bind(dispatch_id)
    .fetch_one(fixture.pool)
    .await
    .unwrap();
    assert_eq!(
        (&audit.0[..], &audit.1[..], &audit.2[..]),
        ("sealed", "sent", "provider_response")
    );
    assert_eq!(audit.3, manifest.digest);
    assert_eq!(audit.4, expected_hash);
    assert_eq!(audit.5, frozen.body);
    assert_eq!(audit.6, raw_response);
    assert_eq!(audit.7, format!("{:x}", Sha256::digest(&audit.6)));
    let usage: (Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT input_tokens,output_tokens FROM advisory_dispatch WHERE workspace_id=$1 AND id=$2",
    )
    .bind(fixture.workspace)
    .bind(dispatch_id)
    .fetch_one(fixture.pool)
    .await
    .unwrap();
    assert_eq!(usage, (Some(20), Some(30)));
    let replay = route(&mut owner, "command", "pipeline.recommendation.run", run).await;
    assert_eq!(replay, ranked);
    assert_no_http(&listener).await;
    let dispatches: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1 AND opportunity_id=$2",
    )
    .bind(fixture.workspace)
    .bind(opportunity)
    .fetch_one(fixture.pool)
    .await
    .unwrap();
    assert_eq!(dispatches, 1);
    if let Some(ready) = ready {
        // Recovery is actor-bound; explicit disposition/caller authority still
        // belongs to the original captured session, which remains lawful.
        owner.finish().await;
        owner = Mcp::start(
            &socket,
            fixture.owner_config,
            &caller_native,
            fixture.workspace_key,
        )
        .await;
        owner.call("open_workspace", json!({})).await;
        let receipt:(Vec<u8>,String,i32,bool,Option<String>,Option<String>,Value)=sqlx::query_as("SELECT response_payload,response_sha256,http_status,response_complete,original_input_tokens,original_output_tokens,original_transport_context FROM advisory_provider_observations WHERE dispatch_id=$1").bind(dispatch_id).fetch_one(fixture.pool).await.unwrap();
        assert_eq!(receipt.0, raw_response);
        assert_eq!(receipt.1, format!("{:x}", Sha256::digest(&raw_response)));
        assert_eq!(receipt.2, 200);
        assert!(receipt.3);
        assert_eq!((receipt.4, receipt.5), (None, None));
        assert_eq!(receipt.6["send_certainty"], "sent");
        assert_eq!(receipt.6["outcome"], "provider_response");
        assert!(receipt.6["provider_failure_code"].is_null());
        let interpretation:(String,String,i32,Value)=sqlx::query_as("SELECT manifest_digest,response_sha256,contract_version,ranking FROM pipeline_advice_interpretations WHERE dispatch_id=$1").bind(dispatch_id).fetch_one(fixture.pool).await.unwrap();
        assert_eq!(interpretation.0, manifest.digest);
        assert_eq!(interpretation.1, receipt.1);
        assert_eq!(interpretation.2, 1);
        let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM advisory_budget_reservations WHERE dispatch_id=$1),(SELECT count(*) FROM advisory_budget_consumptions WHERE dispatch_id=$1)").bind(dispatch_id).fetch_one(fixture.pool).await.unwrap();
        assert_eq!(counts, (1, 1));
        let runtime = PgPool::connect(fixture.runtime_url).await.unwrap();
        let rejected=sqlx::query("UPDATE advisory_provider_observations SET elapsed_ms=elapsed_ms+1 WHERE dispatch_id=$1").bind(dispatch_id).execute(&runtime).await.unwrap_err();
        assert_eq!(
            rejected.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
        assert_no_http(&listener).await;
        super::native_effect::exercise(fixture, &socket, &mut owner, &prepared, &ranked, ready)
            .await;
    }
    route(
        &mut owner,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":2,"mode":"optional",
            "provider_profile_ref":{"id":PROFILE},
            "model_configuration":{"model":MODEL}
        }),
    )
    .await;
    owner.finish().await;
    server.abort();
}

pub(super) async fn assert_no_http(listener: &TcpListener) {
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
}

pub(super) fn native_response(body: &[u8], eligible_ids: &[String]) -> Vec<u8> {
    let request: Value = serde_json::from_slice(body).unwrap();
    let mut answers = serde_json::Map::new();
    for (index, _) in eligible_ids.iter().enumerate() {
        let legend = request["questions"][format!("score_v1_{index}")]["criteria"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(level, label)| (level.to_string(), label.clone()))
            .collect::<serde_json::Map<_, _>>();
        let selected_level = 9 - index;
        let probabilities = (0..10)
            .map(|level| {
                (
                    level.to_string(),
                    json!(if level == selected_level { 1.0 } else { 0.0 }),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        answers.insert(
            format!("score_v1_{index}"),
            json!({"type":"score","score":selected_level,"legend":legend,
                   "probabilities":probabilities,"confidence":0.91}),
        );
    }
    let mut probabilities = eligible_ids
        .iter()
        .enumerate()
        .map(|(index, id)| (id.clone(), json!(if index == 0 { 0.8 } else { 0.0 })))
        .collect::<serde_json::Map<_, _>>();
    probabilities.insert("ABSTAIN".into(), json!(0.2));
    answers.insert(
        "choice_v1".into(),
        json!({"type":"choice","choice":eligible_ids[0],
               "probabilities":probabilities,"confidence":0.8}),
    );
    serde_json::to_vec(&json!({"model":"jev-1.13.0","answers":answers,
        "usage":{"input_tokens":20,"output_tokens":30}}))
    .unwrap()
}
