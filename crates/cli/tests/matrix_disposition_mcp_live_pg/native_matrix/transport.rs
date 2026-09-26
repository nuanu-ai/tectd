use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

pub(super) async fn serve_once(
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
    let mut raw = if matches!(case, Case::Malformed) {
        b"{".to_vec()
    } else {
        response(
            &serde_json::from_slice(&body).unwrap(),
            matches!(case, Case::Abstained),
        )
    };
    // A valid ranking/usage JSON prefix cannot authorize parsing or known cost
    if matches!(case, Case::DuplicateUsage) {
        raw = String::from_utf8(raw)
            .unwrap()
            .replace(
                "\"input_tokens\":20",
                "\"input_tokens\":999999,\"input_tokens\":0",
            )
            .into_bytes();
    }
    // when the HTTP entity is incomplete, even if the retained prefix parses.
    if matches!(case, Case::Oversize) {
        raw.resize(64 * 1024, b' ');
    }
    let advertised = raw.len() + usize::from(matches!(case, Case::Oversize | Case::Truncated));
    let status = if matches!(case, Case::Http500) {
        "500 Internal Server Error"
    } else {
        "200 OK"
    };
    connection.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {advertised}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
    connection.write_all(&raw).await.unwrap();
    connection.flush().await.unwrap();
    if matches!(case, Case::Oversize) {
        connection.write_all(b"x").await.unwrap();
    } else if matches!(case, Case::Truncated) {
        // Let the complete prefix reach the transport before the deliberately
        // short Content-Length connection closes; this is a bounded stub hook.
        tokio::time::sleep(Duration::from_millis(50)).await;
        connection.shutdown().await.unwrap();
    }
    (listener, body, raw)
}
