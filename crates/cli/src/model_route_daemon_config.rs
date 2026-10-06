//! Explicit adviser opt-in; a shared credential alone never enables transport.
use std::time::Duration;
use tect_domain::{AdvisoryModelConfiguration, AdvisoryProviderProfileRef, Error, Result};
use tect_host::{JevModelRouteConfig, JevModelRouteProvider};

pub(super) fn model_route_provider_from_env() -> Result<Option<JevModelRouteProvider>> {
    fn optional(name: &str) -> Result<Option<String>> {
        match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(_) => Err(Error::InvalidConfiguration),
        }
    }
    let endpoint = optional("TECT_JEV_MODEL_ROUTE_ENDPOINT")?;
    let profile = optional("TECT_JEV_MODEL_ROUTE_PROVIDER_PROFILE_ID")?;
    let model = optional("TECT_JEV_MODEL_ROUTE_MODEL")?;
    let expected_response_model = optional("TECT_JEV_MODEL_ROUTE_EXPECTED_RESPONSE_MODEL")?;
    if endpoint.is_none() && profile.is_none() && model.is_none() {
        return pin_response_model(None, expected_response_model);
    }
    let provider = configured(endpoint, profile, model, optional("TYPESAFE_API_KEY")?)?
        .map(|(config, key)| JevModelRouteProvider::new(config, key))
        .transpose()?;
    pin_response_model(provider, expected_response_model)
}

fn pin_response_model(
    provider: Option<JevModelRouteProvider>,
    expected_response_model: Option<String>,
) -> Result<Option<JevModelRouteProvider>> {
    match (provider, expected_response_model) {
        (None, None) => Ok(None),
        (None, Some(_)) => Err(Error::InvalidConfiguration),
        (Some(provider), None) => Ok(Some(provider)),
        (Some(provider), Some(model)) => provider.with_expected_response_model(model).map(Some),
    }
}

fn configured(
    endpoint: Option<String>,
    profile: Option<String>,
    model: Option<String>,
    key: Option<String>,
) -> Result<Option<(JevModelRouteConfig, String)>> {
    if endpoint.is_none() && profile.is_none() && model.is_none() {
        return Ok(None);
    }
    let (Some(endpoint), Some(profile), Some(model), Some(key)) = (endpoint, profile, model, key)
    else {
        return Err(Error::InvalidConfiguration);
    };
    if key.trim().is_empty()
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
    Ok(Some((
        JevModelRouteConfig {
            profile,
            model,
            endpoint: url::Url::parse(&endpoint).map_err(|_| Error::InvalidConfiguration)?,
            timeout: Duration::from_secs(10),
            maximum_request_bytes: 512 * 1024,
            maximum_response_bytes: 64 * 1024,
        },
        key,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_model_route_requires_explicit_complete_valid_tuple() {
        for key in [None, Some("shared-key".into())] {
            assert!(configured(None, None, None, key).unwrap().is_none());
        }
        for (endpoint, profile, model, key) in [
            (
                Some("http://127.0.0.1:1234/v1/systemone".into()),
                None,
                None,
                None,
            ),
            (None, Some("fixture".into()), None, Some("key".into())),
            (None, None, Some("adviser".into()), Some("key".into())),
            (
                Some("http://127.0.0.1:1234/v1/systemone".into()),
                Some("fixture".into()),
                Some("adviser".into()),
                None,
            ),
        ] {
            assert!(configured(endpoint, profile, model, key).is_err());
        }
        for (endpoint, profile, model, key) in [
            (
                "http://example.com/v1/systemone",
                "fixture",
                "adviser",
                "key",
            ),
            (
                "http://localhost:1234/v1/systemone",
                "fixture",
                "adviser",
                "key",
            ),
            ("https://example.com/wrong", "fixture", "adviser", "key"),
            (
                "https://example.com/v1/systemone?x=1",
                "fixture",
                "adviser",
                "key",
            ),
            (
                "https://example.com/v1/systemone",
                "bad profile",
                "adviser",
                "key",
            ),
            (
                "https://example.com/v1/systemone",
                "fixture",
                "bad model",
                "key",
            ),
            (
                "https://example.com/v1/systemone",
                "fixture",
                "adviser",
                " ",
            ),
        ] {
            let result = configured(
                Some(endpoint.into()),
                Some(profile.into()),
                Some(model.into()),
                Some(key.into()),
            )
            .and_then(|v| {
                let (c, k) = v.unwrap();
                JevModelRouteProvider::new(c, k).map(|_| ())
            });
            assert!(matches!(result, Err(Error::InvalidConfiguration)));
        }
        let (config, key) = configured(
            Some("http://127.0.0.1:1234/v1/systemone".into()),
            Some("fixture".into()),
            Some("adviser".into()),
            Some("fixture-key".into()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(config.timeout, Duration::from_secs(10));
        assert_eq!(config.maximum_request_bytes, 512 * 1024);
        assert_eq!(config.maximum_response_bytes, 64 * 1024);
        assert!(JevModelRouteProvider::new(config, key).is_ok());
    }
    #[test]
    fn response_model_pin_requires_enabled_provider_and_valid_exact_identity() {
        assert!(pin_response_model(None, None).unwrap().is_none());
        assert!(matches!(
            pin_response_model(None, Some("jev-1.13.0".into())),
            Err(Error::InvalidConfiguration)
        ));
        for pin in ["", " ", "jev 1.13.0", "jev-1.13.0\n", "jev-1.13.0"] {
            let (config, key) = configured(
                Some("http://127.0.0.1:1/v1/systemone".into()),
                Some("fixture".into()),
                Some("jev-latest".into()),
                Some("fixture-key".into()),
            )
            .unwrap()
            .unwrap();
            let provider = JevModelRouteProvider::new(config, key).unwrap();
            let result = pin_response_model(Some(provider), Some(pin.into()));
            assert_eq!(result.is_ok(), pin == "jev-1.13.0");
        }
    }
}
