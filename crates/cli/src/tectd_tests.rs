use super::*;
use tect_application::{MatrixAdviceProvider, PipelineRecommendationProvider};
use tect_domain::{
    EngineeringMode, PIPELINE_COMPATIBILITY_POLICY_VERSION, PipelineCardCoverage,
    PipelineCompatibilityRule, PipelineKind,
};

fn fixture_policy() -> PipelineCompatibilityPolicy {
    PipelineCompatibilityPolicy {
        version: PIPELINE_COMPATIBILITY_POLICY_VERSION.into(),
        task_id: "fixture-task".into(),
        task_revision: "2".into(),
        catalogue_revision: PIPELINE_RECOMMENDATION_CATALOGUE_REVISION.into(),
        rules: vec![PipelineCompatibilityRule {
            kind: PipelineKind::LightweightTddDevelopment,
            matrix_input_digest: "a".repeat(64),
            allowed_modes: vec![EngineeringMode::Production],
            selected_candidate_ids: vec!["candidate-1".into()],
            card_coverage: vec![PipelineCardCoverage {
                card_id: "EM02-SCOPE@0.1".into(),
                phase_id: "proof".into(),
                obligation_digest: "b".repeat(64),
            }],
        }],
    }
}

#[test]
fn compatibility_snapshot_is_optional_and_strictly_parsed() {
    assert_eq!(parse_pipeline_compatibility_policy(None), Ok(None));
    let fixture = fixture_policy();
    let json = serde_json::to_string(&fixture).unwrap();
    assert_eq!(
        parse_pipeline_compatibility_policy(Some(&json)),
        Ok(Some(fixture.clone()))
    );
    for invalid in ["", "{}", "[]", "null"] {
        assert_eq!(
            parse_pipeline_compatibility_policy(Some(invalid)),
            Err(Error::InvalidConfiguration)
        );
    }
    let mut unknown = serde_json::to_value(&fixture).unwrap();
    unknown["unknown"] = serde_json::json!(true);
    assert_eq!(
        parse_pipeline_compatibility_policy(Some(&unknown.to_string())),
        Err(Error::InvalidConfiguration)
    );
    let mut incomplete = fixture.clone();
    incomplete.rules[0].card_coverage.clear();
    let mut stale = fixture.clone();
    stale.catalogue_revision = "3".into();
    let mut unsupported = fixture.clone();
    unsupported.version = "unknown".into();
    for policy in [incomplete, stale, unsupported] {
        let json = serde_json::to_string(&policy).unwrap();
        assert_eq!(
            parse_pipeline_compatibility_policy(Some(&json)),
            Err(Error::InvalidConfiguration)
        );
    }
}

#[test]
fn database_pool_default_and_bounds_are_stable() {
    assert_eq!(database_max_connections(None), Ok(16));
    assert_eq!(database_max_connections(Some("1")), Ok(1));
    assert_eq!(database_max_connections(Some("64")), Ok(64));
    for invalid in ["", "0", "65", "-1", "1.0", " 16", "16 "] {
        assert_eq!(
            database_max_connections(Some(invalid)),
            Err(Error::InvalidConfiguration)
        );
    }
}

#[test]
fn pipeline_transport_requires_a_complete_explicit_tuple() {
    assert!(
        pipeline_provider_config(None, None, None, None)
            .unwrap()
            .is_none()
    );
    assert!(
        pipeline_provider_config(None, None, None, Some("key".into()))
            .unwrap()
            .is_none()
    );
    let endpoint = Some("https://example.com/v1/systemone".into());
    let profile = Some("workspace-profile".into());
    let model = Some("jev-1.13.0".into());
    for values in [
        (endpoint.clone(), None, None, None),
        (None, profile.clone(), None, Some("key".into())),
        (None, None, model.clone(), Some("key".into())),
        (endpoint.clone(), profile.clone(), model.clone(), None),
        (
            endpoint.clone(),
            profile.clone(),
            model.clone(),
            Some(String::new()),
        ),
    ] {
        assert!(matches!(
            pipeline_provider_config(values.0, values.1, values.2, values.3),
            Err(Error::InvalidConfiguration)
        ));
    }
    let (config, credential) =
        pipeline_provider_config(endpoint, profile, model, Some("fixture-key".into()))
            .unwrap()
            .unwrap();
    assert_eq!(config.identity.provider, "workspace-profile");
    assert_eq!(config.identity.model, "jev-1.13.0");
    assert_eq!(
        config.identity.destination,
        "https://example.com/v1/systemone"
    );
    assert!(
        JevPipelineProvider::new(config, credential)
            .unwrap()
            .available()
    );
    assert!(!tect_application::DisabledPipelineRecommendationProvider.available());
}

#[test]
fn pipeline_transport_rejects_invalid_identity_and_endpoint() {
    for (endpoint, profile, model) in [
        ("not-a-url", "profile", "jev-1"),
        ("http://example.com/v1/systemone", "profile", "jev-1"),
        ("https://example.com/other", "profile", "jev-1"),
        ("https://example.com/v1/systemone?x=1", "profile", "jev-1"),
        ("https://example.com/v1/systemone", "bad profile", "jev-1"),
        ("https://example.com/v1/systemone", "profile", "bad model"),
        ("https://example.com/v1/systemone", "profile", ""),
    ] {
        let result = pipeline_provider_config(
            Some(endpoint.into()),
            Some(profile.into()),
            Some(model.into()),
            Some("fixture-key".into()),
        )
        .and_then(|value| {
            let (config, credential) = value.unwrap();
            JevPipelineProvider::new(config, credential).map(|_| ())
        });
        assert_eq!(result, Err(Error::InvalidConfiguration));
    }
}

#[test]
fn matrix_transport_requires_a_complete_explicit_tuple() {
    assert!(
        matrix_provider_config(None, None, None, None)
            .unwrap()
            .is_none()
    );
    assert!(
        matrix_provider_config(None, None, None, Some("key".into()))
            .unwrap()
            .is_none()
    );
    let endpoint = "https://example.com/v1/systemone";
    for mask in 1..8 {
        let result = matrix_provider_config(
            (mask & 1 != 0).then(|| endpoint.into()),
            (mask & 2 != 0).then(|| "profile".into()),
            (mask & 4 != 0).then(|| "jev-1".into()),
            None,
        );
        assert!(matches!(result, Err(Error::InvalidConfiguration)));
        if mask != 7 {
            let result = matrix_provider_config(
                (mask & 1 != 0).then(|| endpoint.into()),
                (mask & 2 != 0).then(|| "profile".into()),
                (mask & 4 != 0).then(|| "jev-1".into()),
                Some("key".into()),
            );
            assert!(matches!(result, Err(Error::InvalidConfiguration)));
        }
    }
    let (config, credential) = matrix_provider_config(
        Some(endpoint.into()),
        Some("profile".into()),
        Some("jev-1".into()),
        Some("fixture-key".into()),
    )
    .unwrap()
    .unwrap();
    let provider = JevNativeMatrixProvider::new(config, credential).unwrap();
    let identity = provider.identity().unwrap();
    assert_eq!(identity.provider_profile_ref.id, "profile");
    assert_eq!(identity.model_configuration.model, "jev-1");
    assert_eq!(identity.destination, endpoint);
    assert_eq!(identity.wire_version, NATIVE_MATRIX_WIRE_VERSION);
    assert!(
        tect_application::DisabledMatrixAdviceProvider
            .identity()
            .is_none()
    );
}

#[test]
fn matrix_transport_rejects_malformed_endpoint_identity_and_credential() {
    for (endpoint, profile, model, credential) in [
        ("not-a-url", "profile", "jev-1", "key"),
        ("http://example.com/v1/systemone", "profile", "jev-1", "key"),
        ("https://example.com/other", "profile", "jev-1", "key"),
        (
            "https://example.com/v1/systemone?x=1",
            "profile",
            "jev-1",
            "key",
        ),
        (
            "https://example.com/v1/systemone",
            " bad-profile",
            "jev-1",
            "key",
        ),
        (
            "https://example.com/v1/systemone",
            "profile",
            "bad-model ",
            "key",
        ),
        ("https://example.com/v1/systemone", "profile", "jev-1", ""),
        (
            "https://example.com/v1/systemone",
            "profile",
            "jev-1",
            "bad\nkey",
        ),
    ] {
        let result = matrix_provider_config(
            Some(endpoint.into()),
            Some(profile.into()),
            Some(model.into()),
            Some(credential.into()),
        )
        .and_then(|value| {
            let (config, credential) = value.unwrap();
            JevNativeMatrixProvider::new(config, credential)
                .map(|_| ())
                .map_err(|_| Error::InvalidConfiguration)
        });
        assert_eq!(result, Err(Error::InvalidConfiguration));
    }
}
