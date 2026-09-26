use super::*;
use std::{
    os::unix::fs::{FileTypeExt, PermissionsExt},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    process::{Child, Command},
};

pub(super) async fn daemon(
    runtime: &str,
    socket: &std::path::Path,
    endpoint: Option<&str>,
    keys: &Value,
) -> Child {
    // No inherited dotenv, credentials, embedding workers, or other JEV opt-ins.
    let mut command = Command::new(env!("CARGO_BIN_EXE_tectd"));
    command
        .env_clear()
        .env("TECT_DATABASE_URL", runtime)
        .env("TECT_SOCKET", socket)
        .env("TECT_JEV_BUDGET_OWNER_KEYS_JSON", keys.to_string())
        .env("TYPESAFE_API_KEY", "anti-bloat-loopback-fixture-only")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(endpoint) = endpoint {
        assert!(endpoint.starts_with("http://127.0.0.1:"));
        command
            .env("TECT_JEV_ANTI_BLOAT_ENDPOINT", endpoint)
            .env(
                "TECT_JEV_ANTI_BLOAT_PROVIDER_PROFILE_ID",
                "fixture-anti-bloat",
            )
            .env("TECT_JEV_ANTI_BLOAT_MODEL", "fixture-choice-model");
    }
    let mut child = command.spawn().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            assert!(
                child.try_wait().unwrap().is_none(),
                "owned fixture daemon exited"
            );
            if std::fs::symlink_metadata(socket)
                .is_ok_and(|m| m.file_type().is_socket() && m.permissions().mode() & 0o777 == 0o600)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    child
}

pub(super) async fn stop(child: &mut Child) {
    child.kill().await.unwrap();
    child.wait().await.unwrap();
}

pub(super) async fn response(
    listener: TcpListener,
    pool: PgPool,
    review: Uuid,
    status: u16,
    abstain: bool,
    duplicate: bool,
    partial: u8,
) -> (Vec<u8>, Vec<u8>, TcpListener) {
    let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut received = Vec::new();
    let mut chunk = [0u8; 4096];
    let (header_end, length) = loop {
        let read = stream.read(&mut chunk).await.unwrap();
        assert!(read > 0);
        received.extend_from_slice(&chunk[..read]);
        assert!(received.len() < 524_288);
        if let Some(end) = received.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&received[..end]).unwrap();
            assert!(headers.starts_with("POST /v1/systemone HTTP/1.1"));
            assert!(
                headers
                    .to_ascii_lowercase()
                    .contains("authorization: bearer anti-bloat-loopback-fixture-only")
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
    while received.len() < header_end + length {
        let read = stream.read(&mut chunk).await.unwrap();
        assert!(read > 0);
        received.extend_from_slice(&chunk[..read]);
    }
    let request = received[header_end..header_end + length].to_vec();
    let frozen:(String,Vec<u8>,String)=sqlx::query_as("SELECT state,request_bytes,request_sha256 FROM scope_anti_bloat_reviews WHERE review_id=$1").bind(review).fetch_one(&pool).await.unwrap();
    assert_eq!(frozen.0, "sending");
    assert_eq!(frozen.1, request);
    assert_eq!(frozen.2, format!("{:x}", Sha256::digest(&request)));
    let wire: Value = serde_json::from_slice(&request).unwrap();
    let ids = wire["state"]["eligible_ids"].as_array().unwrap();
    assert!(ids.len() >= 2);
    assert_eq!(wire["model"], "fixture-choice-model");
    assert_eq!(wire["state"]["binding"]["review_id"], review.to_string());
    assert_eq!(
        wire["state"]["contract"],
        "tect.anti-bloat-typesafe-choice/1"
    );
    assert_eq!(
        wire["state"]["provider_binding_digest"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    let questions = wire["questions"].as_object().unwrap();
    assert_eq!(questions.len(), 1);
    let question = &questions["anti_bloat_order_v1"];
    assert_eq!(question["type"], "choice");
    let criteria = question["criteria"].as_object().unwrap();
    assert_eq!(criteria.len(), ids.len() + 1);
    for index in 0..ids.len() {
        assert!(criteria.contains_key(&format!("R{index}")));
    }
    assert!(criteria.contains_key("ABSTAIN"));
    let mut probabilities = serde_json::Map::new();
    // Geometrically distinct weights normalized over every candidate + abstain.
    let weights = (0..ids.len())
        .map(|i| 2f64.powi(-(i as i32 + 1)))
        .collect::<Vec<_>>();
    let abstain_weight = if abstain { 2.0 } else { 0.01 };
    let sum = weights.iter().sum::<f64>() + abstain_weight;
    for (index, weight) in weights.iter().enumerate() {
        probabilities.insert(format!("R{index}"), json!(weight / sum));
    }
    probabilities.insert("ABSTAIN".into(), json!(abstain_weight / sum));
    let mut body=serde_json::to_vec(&json!({"model":"fixture-choice-model","answers":{"anti_bloat_order_v1":{"type":"choice","choice":if abstain{"ABSTAIN"}else{"R0"},"probabilities":probabilities,"confidence":0.01}},"usage":{"input_tokens":7,"output_tokens":3}})).unwrap();
    if status == 500 {
        let mut value: Value = serde_json::from_slice(&body).unwrap();
        value["usage"] = json!({"input_tokens":20,"output_tokens":30});
        body = serde_json::to_vec(&value).unwrap();
    }
    if duplicate {
        body = String::from_utf8(body)
            .unwrap()
            .replace(
                "\"input_tokens\":7",
                "\"input_tokens\":999999,\"input_tokens\":0",
            )
            .into_bytes();
    }
    if partial == 1 {
        body.resize(64 * 1024, b' ');
    }
    let header = format!(
        "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len() + usize::from(partial > 0)
    );
    stream.write_all(header.as_bytes()).await.unwrap();
    stream.write_all(&body).await.unwrap();
    stream.flush().await.unwrap();
    if partial == 1 {
        stream.write_all(b"x").await.unwrap();
    }
    if partial == 2 {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    stream.shutdown().await.unwrap();
    (request, body, listener)
}
