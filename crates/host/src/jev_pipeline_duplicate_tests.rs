use super::*;

#[tokio::test]
async fn duplicate_json_retains_raw_with_unknown_usage_and_no_ranking() {
    let native = prepared(2);
    let valid = serde_json::to_string(&response(&native)).unwrap();
    for raw in [
        valid.replace(
            "\"input_tokens\":20",
            "\"input_tokens\":999999,\"input_tokens\":0",
        ),
        valid.replace("\"usage\":", "\"usage\":{},\"usage\":"),
        valid.replace(
            "\"type\":\"choice\"",
            "\"type\":\"score\",\"type\":\"choice\"",
        ),
    ] {
        let bytes = raw.into_bytes();
        assert!(parse_native_response(&bytes, &native, MAX_RESPONSE_BYTES).is_err());
        let (endpoint, captured, server) = http_fixture(200, bytes.clone(), Duration::ZERO).await;
        let observed = http_provider(endpoint, Duration::from_secs(2), MAX_RESPONSE_BYTES)
            .send_once(native.body.clone())
            .await
            .unwrap();
        captured.await.unwrap();
        server.await.unwrap();
        assert_eq!(observed.raw_response, bytes);
        assert_eq!(
            (observed.input_tokens, observed.output_tokens),
            (None, None)
        );
    }
}
