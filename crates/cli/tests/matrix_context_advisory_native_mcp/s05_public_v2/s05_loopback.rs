//! Fresh V2 Matrix/Work lineage, native HTTP on numeric loopback only.
use super::*;

async fn respond_once(
    listener: TcpListener,
    pool: PgPool,
    workspace: Uuid,
    key: String,
    expected: Vec<u8>,
    case: String,
) -> Vec<u8> {
    let (mut stream, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut bytes = Vec::new();
    let mut block = [0u8; 4096];
    let (start, length) = loop {
        let n = stream.read(&mut block).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&block[..n]);
        assert!(bytes.len() < 128 * 1024);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&bytes[..end]).unwrap();
            assert!(headers.starts_with("POST /v1/systemone HTTP/1.1"));
            assert!(
                headers
                    .to_ascii_lowercase()
                    .contains("authorization: bearer s05-loopback-only")
            );
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            break (end + 4, length);
        }
    };
    while bytes.len() < start + length {
        let n = stream.read(&mut block).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&block[..n]);
    }
    let body = &bytes[start..start + length];
    assert_eq!(body, expected);
    let frozen:(String,Vec<u8>,String)=sqlx::query_as("SELECT state,request_payload,request_sha256 FROM model_route_advisory_attempts WHERE workspace_id=$1 AND preparation_request_key=$2")
        .bind(workspace).bind(&key).fetch_one(&pool).await.unwrap();
    assert_eq!(frozen.0, "send_unknown");
    assert_eq!(frozen.1, expected);
    assert_eq!(frozen.2, format!("{:x}", Sha256::digest(body)));
    let (selected, p0, p1, pa, status, tokens) = match case.as_str() {
        "ranked" => ("R1", 0.2, 0.7, 0.1, 200, 3),
        "abstain" => ("ABSTAIN", 0.1, 0.1, 0.8, 200, 3),
        "error" => ("R1", 0.2, 0.7, 0.1, 500, 3),
        "budget" => ("R1", 0.2, 0.7, 0.1, 200, 2500),
        _ => unreachable!(),
    };
    let raw = serde_json::to_vec(&json!({"model":ADVISER,
        "answers":{"model_route_order_v1":{"type":"choice","choice":selected,
            "probabilities":{"R0":p0,"R1":p1,"ABSTAIN":pa},"confidence":0.01}},
        "usage":{"input_tokens":7,"output_tokens":tokens}}))
    .unwrap();
    stream
        .write_all(
            format!(
                "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                raw.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    stream.write_all(&raw).await.unwrap();
    stream.shutdown().await.unwrap();
    raw
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run(
    pool: &PgPool,
    listener: TcpListener,
    endpoint: String,
    owner: &mut Mcp,
    socket: &Path,
    owner_file: &Path,
    workspace_key: &str,
    key: &str,
    workspace: Uuid,
    set: Uuid,
    caller_request: Uuid,
    typed: PreparedModelRouteRecommendation,
) {
    let case =
        std::env::var("S05_LOOPBACK_CASE").expect("select ranked, abstain, error, or budget");
    assert!(matches!(
        case.as_str(),
        "ranked" | "abstain" | "error" | "budget"
    ));
    let dummy_key = "s05-loopback-only";
    let expected = provider_at(&endpoint, dummy_key.into())
        .prepare(&typed)
        .unwrap()
        .request_bytes;
    assert!(expected.len() < MAX_REQUEST);
    let digest = format!("{:x}", Sha256::digest(&expected));
    let http = tokio::spawn(respond_once(
        listener,
        pool.clone(),
        workspace,
        key.to_owned(),
        expected.clone(),
        case.clone(),
    ));
    let raw_run = owner
        .exchange(
            "tools/call",
            recovery_support::public_call(
                "command",
                json!({"route":"model.route.run","params":{"preparation_request_key":key}}),
            ),
        )
        .await;
    let response = http.await.unwrap();
    let observer = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    #[allow(clippy::type_complexity)]
    let audit:(String,Vec<u8>,String,Vec<u8>,String,Option<bool>,Option<Value>,Option<i32>) = sqlx::query_as("SELECT state,request_payload,request_sha256,response_payload,response_sha256,response_complete,original_transport_context,response_http_status FROM model_route_advisory_attempts WHERE workspace_id=$1 AND preparation_request_key=$2")
        .bind(workspace).bind(key).fetch_one(&observer).await.unwrap();
    assert_eq!(audit.1, expected);
    assert_eq!(audit.2, digest);
    assert_eq!(audit.3, response);
    assert_eq!(audit.4, format!("{:x}", Sha256::digest(&audit.3)));
    assert_eq!(audit.5, Some(true));
    assert!(audit.6.is_some());
    let transport = audit.6.as_ref().unwrap();
    assert_eq!(transport["send_certainty"], "sent");
    assert_eq!(transport["raw_response_ref"], format!("sha256:{}", audit.4));
    assert_eq!(
        transport["outcome"],
        if case == "error" {
            "provider_failure"
        } else {
            "provider_response"
        }
    );
    assert_eq!(audit.7, Some(if case == "error" { 500 } else { 200 }));
    let counts:(i64,i64,i64,i64,i64) = sqlx::query_as("SELECT (SELECT coalesce(sum(call_count),0)::bigint FROM advisory_call_audit WHERE workspace_id=$1 AND capability='model_routing'),(SELECT count(*) FROM model_route_budget_reservations WHERE workspace_id=$1),(SELECT count(*) FROM model_route_budget_consumptions WHERE workspace_id=$1),(SELECT count(*) FROM model_route_decisions WHERE workspace_id=$1),(SELECT count(*) FROM model_route_dispositions WHERE workspace_id=$1)")
        .bind(workspace).fetch_one(&observer).await.unwrap();
    assert_eq!((counts.0, counts.1, counts.2), (1, 1, 1));
    let view = if case == "error" {
        assert_eq!(raw_run["result"]["isError"], true);
        let first = recovery_support::tool_payload(&raw_run);
        let replay = route_error(
            owner,
            "command",
            "model.route.run",
            json!({"preparation_request_key":key}),
        )
        .await;
        assert_eq!(replay, first);
        route(
            owner,
            "query",
            "model.route.get",
            json!({"preparation_request_key":key}),
        )
        .await
    } else {
        assert_ne!(raw_run["result"]["isError"], true);
        let first = recovery_support::tool_payload(&raw_run);
        let replay = route(
            owner,
            "command",
            "model.route.run",
            json!({"preparation_request_key":key}),
        )
        .await;
        assert_eq!(first, replay);
        replay
    };
    assert!(view["preparation"]["routes"]["observed_actual"].is_null());
    match case.as_str() {
        "ranked" => {
            assert_eq!(audit.0, "parsed");
            assert_eq!(counts.3, 1);
            assert_eq!(view["decision"]["routes"]["requested_route_id"], "route-a");
            assert_eq!(
                view["decision"]["routes"]["recommended_route_id"],
                "route-b"
            );
            assert!(view["decision"]["routes"]["observed_actual"].is_null());
            let before = route(
                owner,
                "query",
                "model.route.get",
                json!({"preparation_request_key":key}),
            )
            .await;
            assert_eq!(before["decision"]["id"], view["decision"]["id"]);
            let source_session: Uuid = sqlx::query_scalar("SELECT caller_session_id FROM matrix_planning_selection_links WHERE workspace_id=$1 AND candidate_set_id=$2 AND caller_request_id=$3")
                .bind(workspace).bind(set).bind(caller_request).fetch_one(&observer).await.unwrap();
            admin::revoke_session(&observer, source_session)
                .await
                .unwrap();
            let mut fresh = Mcp::start(
                socket,
                owner_file,
                &Uuid::new_v4().to_string(),
                workspace_key,
            )
            .await;
            fresh.call("open_workspace", json!({})).await;
            let disposition = route(&mut fresh,"command","model.route.disposition",json!({"disposition_id":Uuid::new_v4(),"decision_id":view["decision"]["id"],"action":"accept","rationale":"Fresh owner accepts local synthetic recommendation"})).await;
            assert_eq!(disposition["action"], "Accept");
            let saved:(Uuid,Uuid,String,Value) = sqlx::query_as("SELECT d.id,d.decision_id,d.action,x.decision_payload FROM model_route_dispositions d JOIN model_route_decisions x ON (x.tenant_id,x.workspace_id,x.id)=(d.tenant_id,d.workspace_id,d.decision_id) WHERE d.workspace_id=$1 AND d.id=$2")
                .bind(workspace).bind(id(&disposition["id"])).fetch_one(&observer).await.unwrap();
            assert_eq!(
                (saved.0, saved.1, saved.2),
                (
                    id(&disposition["id"]),
                    id(&view["decision"]["id"]),
                    "accept".into()
                )
            );
            // The read-only observer sees the persisted decision independently.
            // Public GET is deliberately bound to the invoking attempt session.
            assert_eq!(saved.3["routes"]["recommended_route_id"], "route-b");
            assert_eq!(saved.3["routes"]["requested_route_id"], "route-a");
            assert!(saved.3["routes"]["observed_actual"].is_null());
            let dispatches: i64 =
                sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1")
                    .bind(workspace)
                    .fetch_one(&observer)
                    .await
                    .unwrap();
            assert_eq!(dispatches, 1, "only the synthetic Matrix adviser dispatch");
            fresh.finish().await;
        }
        "abstain" => {
            assert_eq!(audit.0, "parsed");
            assert_eq!(counts.3, 1);
            assert_eq!(counts.4, 0);
            assert!(
                view["decision"]["outcome"]["Abstained"].is_object(),
                "{view}"
            );
            assert!(view["decision"]["routes"]["recommended_route_id"].is_null());
            assert_eq!(view["decision"]["routes"]["requested_route_id"], "route-a");
            assert!(view["decision"]["routes"]["observed_actual"].is_null());
            assert!(view["disposition"].is_null());
        }
        "error" => {
            assert_eq!(audit.0, "raw_sealed");
            assert_eq!((counts.3, counts.4), (0, 0));
            assert!(view["decision"].is_null());
            assert!(view["disposition"].is_null());
        }
        "budget" => {
            assert_eq!((counts.3, counts.4), (0, 0));
            assert_eq!(view["attempt"]["state"], "budget_exhausted", "{view}");
            assert!(view["decision"].is_null());
            assert!(view["disposition"].is_null());
        }
        _ => unreachable!(),
    }
    let final_dispositions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM model_route_dispositions WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(&observer)
            .await
            .unwrap();
    assert_eq!(final_dispositions, i64::from(case == "ranked"));
    println!(
        "S05 loopback case={case} bytes={} sha256={digest} audit_state={} decisions={} dispositions={} actual=null",
        expected.len(),
        audit.0,
        counts.3,
        final_dispositions
    );
    observer.close().await;
}
