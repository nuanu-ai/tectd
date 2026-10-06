//! Explicit Matrix transport opt-in. Credentials alone never enable dispatch.
use std::time::Duration;
use tect_application::{MAX_PREPARED_MATRIX_BODY_BYTES, MatrixProviderIdentity};
use tect_domain::{AdvisoryModelConfiguration, AdvisoryProviderProfileRef, Error};
use tect_host::jev_matrix_advice::{
    native_provider::{
        JevNativeMatrixConfig, JevNativeMatrixProvider, MAX_NATIVE_MATRIX_RESPONSE_BYTES,
    },
    native_wire::NATIVE_MATRIX_WIRE_VERSION,
};
pub(super) fn matrix_provider_from_env() -> tect_domain::Result<Option<JevNativeMatrixProvider>> {
    fn optional_env(name: &str) -> tect_domain::Result<Option<String>> {
        match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(Error::InvalidConfiguration),
        }
    }
    let endpoint = optional_env("TECT_JEV_MATRIX_ENDPOINT")?;
    let profile = optional_env("TECT_JEV_MATRIX_PROVIDER_PROFILE_ID")?;
    let model = optional_env("TECT_JEV_MATRIX_MODEL")?;
    // A shared TypeSafe key by itself never opts Matrix transport in.
    if endpoint.is_none() && profile.is_none() && model.is_none() {
        return Ok(None);
    }
    let credential = optional_env("TYPESAFE_API_KEY")?;
    matrix_provider_config(endpoint, profile, model, credential)?
        .map(|(config, credential)| {
            JevNativeMatrixProvider::new(config, credential)
                .map_err(|_| Error::InvalidConfiguration)
        })
        .transpose()
}

fn matrix_provider_config(
    endpoint: Option<String>,
    profile: Option<String>,
    model: Option<String>,
    credential: Option<String>,
) -> tect_domain::Result<Option<(JevNativeMatrixConfig, String)>> {
    if endpoint.is_none() && profile.is_none() && model.is_none() {
        return Ok(None);
    }
    let (Some(endpoint), Some(profile), Some(model), Some(credential)) =
        (endpoint, profile, model, credential)
    else {
        return Err(Error::InvalidConfiguration);
    };
    let endpoint = url::Url::parse(&endpoint).map_err(|_| Error::InvalidConfiguration)?;
    let config = JevNativeMatrixConfig {
        provider_identity: MatrixProviderIdentity {
            provider_profile_ref: AdvisoryProviderProfileRef { id: profile },
            model_configuration: AdvisoryModelConfiguration { model },
            destination: endpoint.as_str().into(),
            wire_version: NATIVE_MATRIX_WIRE_VERSION.into(),
            ranking_policy: tect_application::MatrixRankingPolicy::StrictV1,
        },
        endpoint,
        timeout: Duration::from_secs(10),
        maximum_request_bytes: MAX_PREPARED_MATRIX_BODY_BYTES,
        maximum_response_bytes: MAX_NATIVE_MATRIX_RESPONSE_BYTES,
    };
    Ok(Some((config, credential)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_application::MatrixAdviceProvider;
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
        assert_eq!(
            config.maximum_response_bytes,
            MAX_NATIVE_MATRIX_RESPONSE_BYTES
        );
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
}
