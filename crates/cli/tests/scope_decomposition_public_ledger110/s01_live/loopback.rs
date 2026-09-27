use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn response(request: &ScopeAdviceRequest, selective: bool) -> Vec<u8> {
    let mut answers = serde_json::Map::new();
    for (index, alternative) in request.alternatives.iter().enumerate() {
        let preferred = !selective || index == 0;
        let choice = if preferred {
            "PREFERRED"
        } else {
            "NON_PREFERRED"
        };
        let score = if selective && index == 0 { 2.4 } else { 1.2 };
        answers.insert(
            format!("choice_{}", alternative.id.0),
            json!({
                "type":"choice", "choice":choice, "confidence":0.8,
                "probabilities":{"NON_PREFERRED":if preferred {0.2} else {0.8},
                    "PREFERRED":if preferred {0.8} else {0.2}}
            }),
        );
        answers.insert(
            format!("score_{}", alternative.id.0),
            json!({
                "type":"score", "score":score, "confidence":0.7,
                "legend":{"0":"conflict","1":"weak_fit","2":"fit","3":"strong_fit"},
                "probabilities":{"0":0.05,"1":0.1,"2":0.55,"3":0.3}
            }),
        );
    }
    serde_json::to_vec(&json!({"model":MODEL,"answers":answers,
        "usage":{"input_tokens":100,"output_tokens":40}}))
    .unwrap()
}

pub(super) async fn start(
    request: &ScopeAdviceRequest,
    selective: bool,
    fail_status: bool,
) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
    let response_body = response(request, selective);
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 8192];
        let (body_start, length) = loop {
            let count = stream.read(&mut chunk).await.unwrap();
            assert!(count > 0, "request ended before complete HTTP entity");
            bytes.extend_from_slice(&chunk[..count]);
            assert!(bytes.len() <= MAX_REQUEST + 8192);
            if let Some(header_end) = bytes.windows(4).position(|x| x == b"\r\n\r\n") {
                let body_start = header_end + 4;
                let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|value| value.trim().parse().ok())
                    })
                    .expect("request Content-Length required");
                assert!(length <= MAX_REQUEST);
                if bytes.len() >= body_start + length {
                    break (body_start, length);
                }
            }
        };
        let captured = bytes[body_start..body_start + length].to_vec();
        let status = if fail_status {
            "500 Fixture Failure"
        } else {
            "200 OK"
        };
        let header = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response_body.len()
        );
        stream.write_all(header.as_bytes()).await.unwrap();
        stream.write_all(&response_body).await.unwrap();
        stream.flush().await.unwrap();
        captured
    });
    (endpoint, server)
}
