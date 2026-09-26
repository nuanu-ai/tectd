use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn response(request: &Value, none: bool) -> Vec<u8> {
    let alternatives = request["state"]["request"]["alternatives"]
        .as_array()
        .unwrap();
    let mut answers = serde_json::Map::new();
    for (index, alternative) in alternatives.iter().enumerate() {
        let id = alternative["id"].as_str().unwrap();
        let preferred = !none && index == 0;
        let level = if preferred { 3 } else { 1 };
        answers.insert(format!("choice_{id}"),json!({"type":"choice",
            "choice":if preferred{"PREFERRED"}else{"NON_PREFERRED"},"confidence":0.9,
            "probabilities":if preferred{json!({"PREFERRED":0.9,"NON_PREFERRED":0.1})}else{json!({"PREFERRED":0.1,"NON_PREFERRED":0.9})}}));
        let question = format!("score_{id}");
        let legend = request["questions"][&question]["criteria"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, value)| (index.to_string(), value.clone()))
            .collect::<serde_json::Map<_, _>>();
        assert_eq!(
            legend.values().cloned().collect::<Vec<_>>(),
            vec![
                json!("conflict"),
                json!("weak_fit"),
                json!("fit"),
                json!("strong_fit")
            ]
        );
        let probabilities = (0..4)
            .map(|index| {
                (
                    index.to_string(),
                    json!(if index == level { 1.0 } else { 0.0 }),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        answers.insert(question,json!({"type":"score","score":level,"confidence":0.9,"legend":legend,"probabilities":probabilities}));
    }
    serde_json::to_vec(&json!({"model":request["model"],"answers":answers,"usage":{"input_tokens":20,"output_tokens":30}})).unwrap()
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
        let mut buffer = [0u8; 4096];
        let read = connection.read(&mut buffer).await.unwrap();
        assert!(read > 0);
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(index) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
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
        let mut buffer = [0u8; 4096];
        let read = connection.read(&mut buffer).await.unwrap();
        assert!(read > 0);
        bytes.extend_from_slice(&buffer[..read]);
    }
    let body = bytes[head_end..head_end + length].to_vec();
    let sending:(String,String,i64)=sqlx::query_as("SELECT state,send_certainty,(SELECT count(*) FROM advisory_budget_reservations WHERE workspace_id=$1) FROM advisory_dispatch WHERE workspace_id=$1")
        .bind(workspace).fetch_one(&pool).await.unwrap();
    assert_eq!(sending, ("sending".into(), "sent_unknown".into(), 1));
    if matches!(case, Case::Revoked) {
        // Authorized exact disposable-fixture ACL change, no immutable row edits.
        decomposition_parent::guard(&pool).await;
        let changed=sqlx::query("UPDATE agent_sessions SET revoked=true WHERE workspace_id=$1 AND host_id=$2 AND native_session_id=$3 AND NOT revoked")
            .bind(workspace).bind(host).bind(native).execute(&pool).await.unwrap();
        assert_eq!(changed.rows_affected(), 1);
    }
    let mut raw = if matches!(case, Case::Malformed) {
        b"{".to_vec()
    } else {
        response(
            &serde_json::from_slice(&body).unwrap(),
            matches!(case, Case::NoPreference),
        )
    };
    let partial = matches!(case, Case::Partial);
    if matches!(case, Case::DuplicateUsage) {
        let valid = std::str::from_utf8(&raw).unwrap();
        assert!(valid.contains("\"input_tokens\":20"));
        raw = valid
            .replacen(
                "\"input_tokens\":20",
                "\"input_tokens\":999999,\"input_tokens\":0",
                1,
            )
            .into_bytes();
    }
    if matches!(case, Case::InvalidAnswers) {
        let mut value: Value = serde_json::from_slice(&raw).unwrap();
        let answers = value["answers"].as_object_mut().unwrap();
        let required = answers
            .keys()
            .find(|key| key.starts_with("choice_"))
            .unwrap()
            .clone();
        answers.remove(&required);
        raw = serde_json::to_vec(&value).unwrap();
    }
    if partial {
        raw.resize(64 * 1024, b' ');
    }
    let length = raw.len() + usize::from(partial);
    let status = if matches!(case, Case::Http500) {
        "500 Internal Server Error"
    } else {
        "200 OK"
    };
    connection.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
    connection.write_all(&raw).await.unwrap();
    if partial {
        connection.write_all(b"x").await.unwrap();
    }
    (listener, body, raw)
}
