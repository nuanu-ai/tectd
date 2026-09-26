use super::*;
#[test]
fn config_and_source_have_no_hidden_defaults_retries_or_credential_rendering() {
    assert!(
        JevScopeAdviceProvider::new(
            JevScopeAdviceConfig {
                profile: "fixture".into(),
                endpoint: Url::parse("https://api.typesafe.ai/v1/systemone").unwrap(),
                model: "jev-1.13.0".into(),
                timeout: Duration::from_secs(1),
                maximum_request_bytes: 0,
                maximum_response_bytes: 1,
            },
            "credential".into(),
        )
        .is_err()
    );
    assert!(
        JevScopeAdviceProvider::new(
            JevScopeAdviceConfig {
                profile: "".into(),
                endpoint: Url::parse("https://api.typesafe.ai/v1/systemone").unwrap(),
                model: "jev-1.13.0".into(),
                timeout: Duration::from_secs(1),
                maximum_request_bytes: 1,
                maximum_response_bytes: 1,
            },
            "credential".into(),
        )
        .is_err()
    );
    assert!(
        JevScopeAdviceProvider::new(
            JevScopeAdviceConfig {
                profile: "fixture".into(),
                endpoint: Url::parse("https://api.typesafe.ai/v1/systemone?token=hidden").unwrap(),
                model: "jev-1.13.0".into(),
                timeout: Duration::from_secs(1),
                maximum_request_bytes: 1,
                maximum_response_bytes: 1,
            },
            "credential".into(),
        )
        .is_err()
    );
    let source = include_str!("../../jev_scope_advice.rs");
    assert!(!source.contains("pub async fn attempt_prepared("));
    let trait_guard = source
        .find("|| !permit.permits(request.dispatch_id, &prepared)")
        .unwrap();
    let trait_send = source[trait_guard..]
        .find("self.observe_transport(request.dispatch_id, prepared).await")
        .unwrap()
        + trait_guard;
    assert!(trait_guard < trait_send);
    assert_eq!(source.matches(".send()").count(), 1);
    assert!(source.contains(".retry(reqwest::retry::never())"));
    assert!(!source.contains("wire::parse_response("));
    assert!(!source.contains("TYPESAFE_API_KEY"));
    assert!(!source.contains("api.typesafe.ai"));
    assert!(!source.contains("jev-1.13.0"));
    assert!(!source.contains("#[derive(Debug)]\npub struct JevScopeAdviceProvider"));
}
