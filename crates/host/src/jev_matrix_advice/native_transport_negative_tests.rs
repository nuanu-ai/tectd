//! Controlled loopback transport negatives; no credentials or remote endpoint.
use super::*;
use tect_domain::{AdvisoryModelConfiguration, AdvisoryProviderProfileRef};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn fixture(endpoint: Url, timeout: Duration) -> JevNativeMatrixProvider {
    JevNativeMatrixProvider::new(
        JevNativeMatrixConfig {
            provider_identity: MatrixProviderIdentity {
                provider_profile_ref: AdvisoryProviderProfileRef {
                    id: "synthetic-transport".into(),
                },
                model_configuration: AdvisoryModelConfiguration {
                    model: "jev-1.13.0".into(),
                },
                destination: endpoint.to_string(),
                wire_version: native_wire::NATIVE_MATRIX_WIRE_VERSION.into(),
                ranking_policy: MatrixRankingPolicy::StrictV1,
            },
            endpoint,
            timeout,
            maximum_request_bytes: 4096,
            maximum_response_bytes: 1024,
        },
        "synthetic-local-key".into(),
    )
    .unwrap()
}

#[tokio::test]
async fn redirect_is_retained_and_never_followed_or_retried() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let endpoint = Url::parse(&format!("http://{address}/v1/systemone")).unwrap();
    let fake = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 4096];
        assert!(stream.read(&mut bytes).await.unwrap() > 0);
        stream.write_all(format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{address}/v1/systemone\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        drop(stream);
        assert!(
            tokio::time::timeout(Duration::from_millis(150), listener.accept())
                .await
                .is_err()
        );
    });
    let observed = fixture(endpoint, Duration::from_secs(1))
        .send_once(b"{}".to_vec())
        .await
        .unwrap();
    assert_eq!(observed.http_status, Some(307));
    assert_eq!(
        observed
            .original_transport_context
            .unwrap()
            .provider_failure_code
            .as_deref(),
        Some("http-status")
    );
    fake.await.unwrap();
}

#[tokio::test]
async fn timeout_has_unknown_send_and_no_transport_retry() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = Url::parse(&format!(
        "http://{}/v1/systemone",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let fake = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 4096];
        assert!(stream.read(&mut bytes).await.unwrap() > 0);
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(150), listener.accept())
                .await
                .is_err()
        );
    });
    let start = std::time::Instant::now();
    let observed = fixture(endpoint, Duration::from_millis(40))
        .send_once(b"{}".to_vec())
        .await;
    assert_eq!(observed, Err(Error::TransportUnavailable));
    assert!(start.elapsed() < Duration::from_secs(1));
    fake.await.unwrap();
}
