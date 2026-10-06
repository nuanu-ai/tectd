//! Controlled numeric-loopback native Choice fixture only.
use tokio::io::{AsyncReadExt, AsyncWriteExt};
type Fake = (
    reqwest::Url,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<(Vec<u8>, Vec<u8>, bool)>,
);
pub(super) async fn once() -> Fake {
    controlled(None).await
}
pub(super) async fn held() -> (Fake, tokio::sync::oneshot::Sender<()>) {
    let (release, hold) = tokio::sync::oneshot::channel();
    (controlled(Some(hold)).await, release)
}
async fn controlled(hold: Option<tokio::sync::oneshot::Receiver<()>>) -> Fake {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = reqwest::Url::parse(&format!(
        "http://{}/v1/systemone",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let (done, complete) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        let end = loop {
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buf[..n]);
            if let Some(p) = request.windows(4).position(|v| v == b"\r\n\r\n") {
                break p + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..end]);
        let length: usize = headers
            .lines()
            .find_map(|line| {
                let (k, v) = line.split_once(':')?;
                k.eq_ignore_ascii_case("content-length")
                    .then(|| v.trim().parse().unwrap())
            })
            .unwrap();
        while request.len() < end + length {
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buf[..n]);
        }
        let body = request[end..].to_vec();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["model"], "jev");
        assert!(parsed.get("max_tokens").is_none());
        if let Some(hold) = hold {
            hold.await.unwrap();
        }
        let response=serde_json::to_vec(&serde_json::json!({"model":"jev","answers":{"anti_bloat_order_v1":{"type":"choice","choice":"R0","probabilities":{"R0":0.9,"ABSTAIN":0.1}}},"usage":{"input_tokens":20,"output_tokens":30}})).unwrap();
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        );
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(&response).await.unwrap();
        drop(socket);
        let second = tokio::select! {biased; v=listener.accept()=>v.is_ok(), _=complete=>false};
        (body, response, second)
    });
    (endpoint, done, server)
}
