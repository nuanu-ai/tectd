use super::*;

pub(super) struct FixtureResponse {
    pub(super) status: u16,
    pub(super) content_type: &'static str,
    pub(super) body: Vec<u8>,
    pub(super) delay: Duration,
}

pub(super) async fn fixture(
    response: FixtureResponse,
) -> (
    Url,
    Arc<AtomicUsize>,
    oneshot::Receiver<Vec<u8>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let server_calls = calls.clone();
    let (sender, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        server_calls.fetch_add(1, Ordering::SeqCst);
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let header_end = loop {
            let count = socket.read(&mut buffer).await.unwrap();
            if count == 0 {
                return;
            }
            request.extend_from_slice(&buffer[..count]);
            if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        while request.len() < header_end + length {
            let count = socket.read(&mut buffer).await.unwrap();
            if count == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..count]);
        }
        let _ = sender.send(request);
        tokio::time::sleep(response.delay).await;
        let reason = if response.status == 200 {
            "OK"
        } else {
            "ERROR"
        };
        let head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.status,
            reason,
            response.content_type,
            response.body.len()
        );
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(&response.body).await.unwrap();
    });
    (
        Url::parse(&format!("http://{address}/v1/systemone")).unwrap(),
        calls,
        receiver,
        task,
    )
}

pub(super) fn provider(endpoint: Url, timeout: Duration, maximum: usize) -> JevScopeAdviceProvider {
    provider_with_caps(endpoint, timeout, usize::MAX, maximum)
}

pub(super) fn provider_with_caps(
    endpoint: Url,
    timeout: Duration,
    maximum_request_bytes: usize,
    maximum_response_bytes: usize,
) -> JevScopeAdviceProvider {
    JevScopeAdviceProvider::new(
        JevScopeAdviceConfig {
            profile: "fixture".into(),
            endpoint,
            model: "jev-1.13.0".into(),
            timeout,
            maximum_request_bytes,
            maximum_response_bytes,
        },
        "secret-fixture-credential".into(),
    )
    .unwrap()
}
