use std::time::Duration;
use tect_domain::{AdvisoryModelConfiguration, AdvisoryProviderProfileRef, Error};
use tect_host::{JevScopeAdviceConfig, JevScopeAdviceProvider};

pub(super) fn database_max_connections(value: Option<&str>) -> tect_domain::Result<u32> {
    match value {
        None => Ok(16),
        Some(value) => value
            .parse::<u32>()
            .ok()
            .filter(|value| (1..=64).contains(value))
            .ok_or(Error::InvalidConfiguration),
    }
}

pub(super) fn scope_provider_from_env() -> tect_domain::Result<Option<JevScopeAdviceProvider>> {
    fn optional_env(name: &str) -> tect_domain::Result<Option<String>> {
        match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(Error::InvalidConfiguration),
        }
    }
    let endpoint = optional_env("TECT_JEV_SCOPE_ENDPOINT")?;
    let profile = optional_env("TECT_JEV_SCOPE_PROVIDER_PROFILE_ID")?;
    let model = optional_env("TECT_JEV_SCOPE_MODEL")?;
    if endpoint.is_none() && profile.is_none() && model.is_none() {
        return Ok(None);
    }
    let credential = optional_env("TYPESAFE_API_KEY")?;
    scope_provider_config(endpoint, profile, model, credential)?
        .map(|(config, credential)| JevScopeAdviceProvider::new(config, credential))
        .transpose()
}

fn scope_provider_config(
    endpoint: Option<String>,
    profile: Option<String>,
    model: Option<String>,
    credential: Option<String>,
) -> tect_domain::Result<Option<(JevScopeAdviceConfig, String)>> {
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
    let loopback_http = match endpoint.host() {
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        _ => false,
    };
    if (endpoint.scheme() != "https" && !(endpoint.scheme() == "http" && loopback_http))
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(Some((
        JevScopeAdviceConfig {
            profile,
            endpoint,
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
    fn scope_transport_requires_complete_explicit_tuple() {
        assert!(
            scope_provider_config(None, None, None, None)
                .unwrap()
                .is_none()
        );
        assert!(
            scope_provider_config(None, None, None, Some("key".into()))
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
            (
                endpoint.clone(),
                profile.clone(),
                model.clone(),
                Some("   ".into()),
            ),
        ] {
            assert!(matches!(
                scope_provider_config(values.0, values.1, values.2, values.3),
                Err(Error::InvalidConfiguration)
            ));
        }
        let (config, credential) =
            scope_provider_config(endpoint, profile, model, Some("fixture-key".into()))
                .unwrap()
                .unwrap();
        assert_eq!(config.profile, "workspace-profile");
        assert_eq!(config.model, "jev-1.13.0");
        assert_eq!(config.timeout, Duration::from_secs(10));
        assert_eq!(config.maximum_request_bytes, 512 * 1024);
        assert_eq!(config.maximum_response_bytes, 64 * 1024);
        assert!(JevScopeAdviceProvider::new(config, credential).is_ok());
    }

    #[test]
    fn scope_transport_rejects_invalid_identity_and_endpoint() {
        for (endpoint, profile, model) in [
            ("not-a-url", "profile", "jev-1"),
            ("http://example.com/v1/systemone", "profile", "jev-1"),
            ("http://localhost/v1/systemone", "profile", "jev-1"),
            ("https://user@example.com/v1/systemone", "profile", "jev-1"),
            ("https://example.com/v1/systemone?x=1", "profile", "jev-1"),
            (
                "https://example.com/v1/systemone#fragment",
                "profile",
                "jev-1",
            ),
            ("https://example.com/v1/systemone", "bad profile", "jev-1"),
            ("https://example.com/v1/systemone", "profile", "bad model"),
            ("https://example.com/v1/systemone", "profile", ""),
        ] {
            let result = scope_provider_config(
                Some(endpoint.into()),
                Some(profile.into()),
                Some(model.into()),
                Some("fixture-key".into()),
            )
            .and_then(|value| {
                let (config, credential) = value.unwrap();
                JevScopeAdviceProvider::new(config, credential).map(|_| ())
            });
            assert!(
                matches!(result, Err(Error::InvalidConfiguration)),
                "{endpoint}"
            );
        }
        for endpoint in [
            "http://127.0.0.1:1234/v1/systemone",
            "http://[::1]:1234/v1/systemone",
        ] {
            let (config, credential) = scope_provider_config(
                Some(endpoint.into()),
                Some("profile".into()),
                Some("jev-1".into()),
                Some("fixture-key".into()),
            )
            .unwrap()
            .unwrap();
            assert!(JevScopeAdviceProvider::new(config, credential).is_ok());
        }
    }
}
