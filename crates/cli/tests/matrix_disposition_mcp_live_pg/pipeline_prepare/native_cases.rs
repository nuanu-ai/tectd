//! Received native bytes are durable facts, not advice or automatic effects.
use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
type RawAudit = (
    Vec<u8>,
    String,
    i32,
    bool,
    Option<String>,
    Option<String>,
    String,
    Value,
);

#[derive(Clone, Copy, Debug)]
pub(super) enum Case {
    Http500,
    NonJson,
    Empty,
    Malformed,
    InvalidAnswer,
    Partial,
    Duplicate,
    Revoked,
}
pub(super) const CASES: [Case; 8] = [
    Case::Http500,
    Case::NonJson,
    Case::Empty,
    Case::Malformed,
    Case::InvalidAnswer,
    Case::Partial,
    Case::Duplicate,
    Case::Revoked,
];

pub(super) async fn exercise(
    fixture: &NoCallFixture<'_>,
    socket: &std::path::Path,
    mut listener: TcpListener,
    case: Case,
) -> TcpListener {
    {
        let native = Uuid::new_v4().to_string();
        let mut owner =
            Mcp::start(socket, fixture.owner_config, &native, fixture.workspace_key).await;
        owner.call("open_workspace", json!({})).await;
        let revision: i64 = sqlx::query_scalar(
            "SELECT revision FROM slice_candidate_sets WHERE workspace_id=$1 AND id=$2",
        )
        .bind(fixture.workspace)
        .bind(fixture.set)
        .fetch_one(fixture.pool)
        .await
        .unwrap();
        let prepared=route(&mut owner,"command","pipeline.recommendation.prepare",json!({
            "candidate_set_id":fixture.set,"expected_candidate_set_revision":revision,
            "work_node_id":fixture.work["id"],"expected_work_node_revision":fixture.work["revision"],
            "request_key":format!("native-raw-{case:?}-{}",Uuid::new_v4())})).await;
        assert_eq!(prepared["state"], "prepared");
        let opportunity = Uuid::parse_str(prepared["opportunity_id"].as_str().unwrap()).unwrap();
        let manifest:Value=sqlx::query_scalar("SELECT manifest_payload FROM pipeline_advice_contexts WHERE workspace_id=$1 AND opportunity_id=$2").bind(fixture.workspace).bind(opportunity).fetch_one(fixture.pool).await.unwrap();
        let manifest: PipelineRecommendationManifest = serde_json::from_value(manifest).unwrap();
        let frozen = tect_host::jev_pipeline_recommendation::prepare_native_request(
            "jev-1.13.0",
            &manifest,
            tect_host::jev_pipeline_recommendation::MAX_REQUEST_BYTES,
        )
        .unwrap();
        let mut raw = super::http_public::native_response(&frozen.body, &frozen.eligible_ids);
        match case {
            Case::Empty => raw.clear(),
            Case::Malformed => raw = b"{".to_vec(),
            Case::InvalidAnswer => {
                let mut body: Value = serde_json::from_slice(&raw).unwrap();
                body["answers"].as_object_mut().unwrap().remove("choice_v1");
                raw = serde_json::to_vec(&body).unwrap();
            }
            Case::Partial => raw.resize(64 * 1024, b' '),
            Case::Duplicate => {
                raw = String::from_utf8(raw)
                    .unwrap()
                    .replace(
                        "\"input_tokens\":20",
                        "\"input_tokens\":999999,\"input_tokens\":0",
                    )
                    .into_bytes()
            }
            _ => {}
        }
        let status = if matches!(case, Case::Http500) {
            500
        } else {
            200
        };
        let pool = fixture.pool.clone();
        let workspace = fixture.workspace;
        let native_for_stub = native.clone();
        let response = raw.clone();
        let stub = tokio::spawn(async move {
            let (mut stream, _) =
                tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept())
                    .await
                    .expect("fresh authorized fixture must send one POST")
                    .unwrap();
            let pending:(String,i64)=sqlx::query_as("SELECT state,(SELECT count(*) FROM advisory_budget_reservations WHERE dispatch_id=d.id) FROM advisory_dispatch d WHERE workspace_id=$1 AND opportunity_id=$2").bind(workspace).bind(opportunity).fetch_one(&pool).await.unwrap();
            assert_eq!(pending, ("sending".into(), 1));
            let mut bytes = Vec::new();
            let mut chunk = [0; 4096];
            let (start, length) = loop {
                let n = stream.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&bytes[..i]).unwrap();
                    assert!(headers.contains("Bearer fixture-secret"));
                    let length = headers
                        .lines()
                        .find_map(|l| {
                            l.split_once(':')
                                .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                                .map(|(_, v)| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    break (i + 4, length);
                }
            };
            while bytes.len() < start + length {
                let n = stream.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
            }
            if matches!(case, Case::Revoked) {
                let changed=sqlx::query("UPDATE agent_sessions SET revoked=true WHERE workspace_id=$1 AND native_session_id=$2 AND NOT revoked").bind(workspace).bind(native_for_stub).execute(&pool).await.unwrap();
                assert_eq!(changed.rows_affected(), 1);
            }
            let content = if matches!(case, Case::NonJson) {
                "text/plain"
            } else {
                "application/json"
            };
            let advertised = response.len() + usize::from(matches!(case, Case::Partial));
            stream.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: {content}\r\nContent-Length: {advertised}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            stream.write_all(&response).await.unwrap();
            if matches!(case, Case::Partial) {
                stream.write_all(b"x").await.unwrap();
            }
            stream.shutdown().await.unwrap();
            (listener, bytes[start..start + length].to_vec())
        });
        let request = json!({"opportunity_id":opportunity});
        let result = public_run(&mut owner, request.clone()).await;
        let (returned, sent) = stub.await.unwrap();
        listener = returned;
        assert_eq!(sent, frozen.body);
        if matches!(case, Case::Revoked) {
            assert_error(&result, &["session_revoked"]);
        } else {
            assert_ne!(result["status"], "ranked", "{result}");
            let replay = public_run(&mut owner, request).await;
            assert_eq!(replay, result);
        }
        super::http_public::assert_no_http(&listener).await;
        audit(fixture, opportunity, &raw, status, case).await;
        owner.finish().await;
        println!(
            "native Pipeline {case:?}: one POST, raw retained, one reservation/consumption, no advice/effects or replay HTTP"
        );
    }
    listener
}

async fn public_run(owner: &mut Mcp, request: Value) -> Value {
    let response = owner
        .exchange(
            "tools/call",
            recovery_support::public_call(
                "command",
                json!({
        "route":"pipeline.recommendation.run","params":request}),
            ),
        )
        .await;
    recovery_support::tool_payload(&response)
}

async fn audit(
    fixture: &NoCallFixture<'_>,
    opportunity: Uuid,
    raw: &[u8],
    status: i32,
    case: Case,
) {
    let row:RawAudit=sqlx::query_as("SELECT response_payload,response_sha256,http_status,response_complete,original_input_tokens,original_output_tokens,original_transport_outcome,original_transport_context FROM advisory_provider_observations WHERE workspace_id=$1 AND opportunity_id=$2").bind(fixture.workspace).bind(opportunity).fetch_one(fixture.pool).await.unwrap();
    assert_eq!(row.0, raw);
    assert_eq!(row.1, format!("{:x}", Sha256::digest(raw)));
    assert_eq!(row.2, status);
    assert_eq!(row.3, !matches!(case, Case::Partial));
    assert_eq!((row.4, row.5), (None, None));
    assert_eq!(
        row.6,
        if matches!(case, Case::Partial) {
            "partial_received"
        } else {
            "received"
        }
    );
    assert_eq!(row.7["send_certainty"], "sent");
    let failure = match case {
        Case::Http500 => Some("http-status"),
        Case::NonJson => Some("content-type"),
        Case::Empty => Some("empty-response"),
        Case::Partial => Some("response-oversize"),
        _ => None,
    };
    assert_eq!(
        row.7["outcome"],
        if failure.is_some() {
            "provider_failure"
        } else {
            "provider_response"
        }
    );
    assert_eq!(row.7["provider_failure_code"], json!(failure));
    let unknown = matches!(
        case,
        Case::Empty | Case::Malformed | Case::Partial | Case::Duplicate
    );
    let counts:(i64,i64,i64,Option<i64>,Option<i64>,bool)=sqlx::query_as("SELECT (SELECT count(*) FROM advisory_dispatch WHERE opportunity_id=$1),(SELECT count(*) FROM advisory_budget_reservations r JOIN advisory_dispatch d ON d.id=r.dispatch_id WHERE d.opportunity_id=$1),(SELECT count(*) FROM advisory_budget_consumptions c JOIN advisory_dispatch d ON d.id=c.dispatch_id WHERE d.opportunity_id=$1),c.input_tokens,c.output_tokens,c.unknown_usage FROM advisory_budget_consumptions c JOIN advisory_dispatch d ON d.id=c.dispatch_id WHERE d.opportunity_id=$1").bind(opportunity).fetch_one(fixture.pool).await.unwrap();
    assert_eq!((counts.0, counts.1, counts.2), (1, 1, 1));
    assert_eq!(
        (counts.3, counts.4, counts.5),
        if unknown {
            (None, None, true)
        } else {
            (Some(20), Some(30), false)
        }
    );
    let effects:(i64,i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM pipeline_advice_interpretations WHERE opportunity_id=$1),(SELECT count(*) FROM native_slices WHERE workspace_id=$2),(SELECT count(*) FROM slice_pipeline_runs WHERE workspace_id=$2),(SELECT count(*) FROM slice_pipeline_phase_attempts WHERE workspace_id=$2)").bind(opportunity).bind(fixture.workspace).fetch_one(fixture.pool).await.unwrap();
    assert_eq!(effects, (0, 0, 0, 0));
}
