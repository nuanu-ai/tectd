use std::time::Duration;
use tect_domain::{AdvisoryModelConfiguration, AdvisoryProviderProfileRef, Error};
use tect_host::{JevAntiBloatConfig, JevAntiBloatProvider};

pub(super) fn anti_bloat_provider_from_env() -> tect_domain::Result<Option<JevAntiBloatProvider>> {
    fn optional_env(name: &str) -> tect_domain::Result<Option<String>> {
        match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(_) => Err(Error::InvalidConfiguration),
        }
    }
    let endpoint = optional_env("TECT_JEV_ANTI_BLOAT_ENDPOINT")?;
    let profile = optional_env("TECT_JEV_ANTI_BLOAT_PROVIDER_PROFILE_ID")?;
    let model = optional_env("TECT_JEV_ANTI_BLOAT_MODEL")?;
    if endpoint.is_none() && profile.is_none() && model.is_none() {
        return Ok(None);
    }
    anti_bloat_provider_config(endpoint, profile, model, optional_env("TYPESAFE_API_KEY")?)?
        .map(|(config, key)| JevAntiBloatProvider::new(config, key))
        .transpose()
}

fn anti_bloat_provider_config(
    endpoint: Option<String>,
    profile: Option<String>,
    model: Option<String>,
    credential: Option<String>,
) -> tect_domain::Result<Option<(JevAntiBloatConfig, String)>> {
    if endpoint.is_none() && profile.is_none() && model.is_none() {
        return Ok(None);
    }
    let (Some(endpoint), Some(profile), Some(model), Some(credential)) =
        (endpoint, profile, model, credential)
    else {
        return Err(Error::InvalidConfiguration);
    };
    if credential.trim().is_empty()
        || profile.chars().any(char::is_whitespace)
        || model.chars().any(char::is_whitespace)
    {
        return Err(Error::InvalidConfiguration);
    }
    AdvisoryProviderProfileRef {
        id: profile.clone(),
    }
    .validate()
    .map_err(|_| Error::InvalidConfiguration)?;
    AdvisoryModelConfiguration {
        model: model.clone(),
    }
    .validate()
    .map_err(|_| Error::InvalidConfiguration)?;
    let endpoint = url::Url::parse(&endpoint).map_err(|_| Error::InvalidConfiguration)?;
    // Construction performs the shared bounded HTTPS/numeric-loopback checks.
    Ok(Some((
        JevAntiBloatConfig {
            endpoint,
            profile,
            model,
            timeout: Duration::from_secs(10),
            maximum_request_bytes: 512 * 1024,
            maximum_response_bytes: 64 * 1024,
        },
        credential,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anti_bloat_transport_is_explicit_complete_and_validated() {
        assert!(
            anti_bloat_provider_config(None, None, None, None)
                .unwrap()
                .is_none()
        );
        assert!(
            anti_bloat_provider_config(None, None, None, Some("fixture-key".into()))
                .unwrap()
                .is_none()
        );
        let endpoint = Some("http://127.0.0.1:1234/v1/systemone".into());
        let profile = Some("fixture-profile".into());
        let model = Some("jev-fixture".into());
        for tuple in [
            (
                endpoint.clone(),
                None,
                model.clone(),
                Some("fixture-key".into()),
            ),
            (
                None,
                profile.clone(),
                model.clone(),
                Some("fixture-key".into()),
            ),
            (
                endpoint.clone(),
                profile.clone(),
                None,
                Some("fixture-key".into()),
            ),
            (endpoint.clone(), profile.clone(), model.clone(), None),
            (
                endpoint.clone(),
                profile.clone(),
                model.clone(),
                Some(" ".into()),
            ),
        ] {
            assert!(matches!(
                anti_bloat_provider_config(tuple.0, tuple.1, tuple.2, tuple.3),
                Err(Error::InvalidConfiguration)
            ));
        }
        let (config, key) =
            anti_bloat_provider_config(endpoint, profile, model, Some("fixture-key".into()))
                .unwrap()
                .unwrap();
        assert_eq!(config.timeout, Duration::from_secs(10));
        assert_eq!(config.maximum_request_bytes, 512 * 1024);
        assert_eq!(config.maximum_response_bytes, 64 * 1024);
        assert!(JevAntiBloatProvider::new(config, key).is_ok());
        for url in [
            "not-url",
            "http://example.com/v1/systemone",
            "http://localhost/v1/systemone",
            "https://example.com/wrong",
            "https://example.com/v1/systemone?x=1",
            "https://user@example.com/v1/systemone",
        ] {
            assert!(
                anti_bloat_provider_config(
                    Some(url.into()),
                    Some("fixture".into()),
                    Some("jev".into()),
                    Some("fixture-key".into())
                )
                .and_then(|value| {
                    let (config, key) = value.unwrap();
                    JevAntiBloatProvider::new(config, key).map(|_| ())
                })
                .is_err()
            );
        }
        for (profile, model) in [
            ("bad profile", "jev"),
            ("fixture", "bad model"),
            ("fixture", ""),
        ] {
            assert!(
                anti_bloat_provider_config(
                    Some("https://example.com/v1/systemone".into()),
                    Some(profile.into()),
                    Some(model.into()),
                    Some("fixture-key".into())
                )
                .is_err()
            );
        }
    }
}
