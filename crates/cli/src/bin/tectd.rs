use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tect_application::{PipelineProviderIdentity, WorkspaceService};
use tect_domain::{AdvisoryModelConfiguration, AdvisoryProviderProfileRef, Error};
use tect_host::jev_pipeline_recommendation::{
    JevPipelineConfig, JevPipelineProvider, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, WIRE_VERSION,
};
use tect_postgres::{
    BudgetOwnerKeys, PgScopeAuthoredManifestSupplier, PgScopeAuthorityObserver, PgStore,
};
use tokio::net::UnixListener;

#[path = "../knowledge_search_worker.rs"]
mod knowledge_search_worker;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{}", error.code());
        std::process::exit(1);
    }
}

async fn run() -> tect_domain::Result<()> {
    let socket = required_absolute_socket()?;
    let database_url = required_env("TECT_DATABASE_URL")?;
    let max_connections = database_max_connections(
        std::env::var("TECT_DATABASE_MAX_CONNECTIONS")
            .ok()
            .as_deref(),
    )?;
    let budget_owner_keys = match std::env::var("TECT_JEV_BUDGET_OWNER_KEYS_JSON") {
        Ok(value) => BudgetOwnerKeys::from_json(&value)?,
        Err(std::env::VarError::NotPresent) => BudgetOwnerKeys::default(),
        Err(_) => return Err(Error::InvalidConfiguration),
    };
    let pipeline_provider = pipeline_provider_from_env()?;
    validate_socket_parent(&socket)?;
    reject_existing_path(&socket)?;

    let store = PgStore::connect(&database_url, max_connections)
        .await?
        .with_budget_owner_keys(budget_owner_keys);
    let authority = Arc::new(PgScopeAuthorityObserver::new(
        store.clone(),
        Arc::new(tect_host::StaticCandidateGuidance),
    ));
    let supplier = Arc::new(PgScopeAuthoredManifestSupplier::new(
        store.clone(),
        authority.clone(),
    ));
    let mut service = WorkspaceService::new_with_scope_sources(
        Arc::new(store),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
        authority,
        supplier,
    )
    .with_pipeline_recommendation_definitions(Arc::new(
        tect_host::StaticPipelineRecommendationDefinitions,
    ));
    if let Some(provider) = pipeline_provider {
        service = service.with_pipeline_recommendation_provider(Arc::new(provider));
    }
    if let Some(catalogue) = tect_host::StaticModelRouteCatalogue::from_env()? {
        service = service.with_model_route_catalogue_provider(Arc::new(catalogue));
    }
    if let Some(capabilities) = tect_host::StaticModelRouteHostCapabilities::from_env()? {
        service = service.with_model_route_host_capabilities_provider(Arc::new(capabilities));
    }
    let embedding_enabled = match tect_host::LocalEmbeddingConfig::from_env() {
        Ok(Some(config)) => {
            service = service.with_knowledge_embedding_provider(Arc::new(
                tect_host::LocalKnowledgeEmbeddingWorker::new(config),
            ));
            true
        }
        Ok(None) | Err(_) => false,
    };
    let service = Arc::new(service);
    let maintenance_enabled = knowledge_search_worker::maintenance_enabled()?;
    let knowledge_worker = if embedding_enabled || maintenance_enabled {
        let config = knowledge_search_worker::SearchWorkerConfig::from_env()?;
        if maintenance_enabled && config.is_none() {
            return Err(Error::InvalidConfiguration);
        }
        config.map(|config| {
            tokio::spawn(knowledge_search_worker::run(
                service.clone(),
                config,
                embedding_enabled,
                maintenance_enabled,
            ))
        })
    } else {
        None
    };
    let listener = UnixListener::bind(&socket).map_err(|_| Error::InvalidConfiguration)?;
    let guard = SocketGuard::capture(socket)?;
    guard.set_private()?;

    let mut server = tokio::spawn(tect_host::serve(listener, service));
    tokio::select! {
        result = &mut server => {
            if let Some(worker) = &knowledge_worker { worker.abort(); }
            result.map_err(|_| Error::TransportUnavailable)?
        }
        signal = shutdown_signal() => {
            signal?;
            server.abort();
            if let Some(worker) = &knowledge_worker { worker.abort(); }
            let _ = server.await;
            Ok(())
        }
    }
}

fn database_max_connections(value: Option<&str>) -> tect_domain::Result<u32> {
    match value {
        None => Ok(16),
        Some(value) => value
            .parse::<u32>()
            .ok()
            .filter(|value| (1..=64).contains(value))
            .ok_or(Error::InvalidConfiguration),
    }
}

fn pipeline_provider_from_env() -> tect_domain::Result<Option<JevPipelineProvider>> {
    fn optional_env(name: &str) -> tect_domain::Result<Option<String>> {
        match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(Error::InvalidConfiguration),
        }
    }
    let endpoint = optional_env("TECT_JEV_PIPELINE_ENDPOINT")?;
    let profile = optional_env("TECT_JEV_PIPELINE_PROVIDER_PROFILE_ID")?;
    let model = optional_env("TECT_JEV_PIPELINE_MODEL")?;
    // An existing TypeSafe key alone never opts the pipeline transport in.
    if endpoint.is_none() && profile.is_none() && model.is_none() {
        return Ok(None);
    }
    let credential = optional_env("TYPESAFE_API_KEY")?;
    pipeline_provider_config(endpoint, profile, model, credential)?
        .map(|(config, credential)| JevPipelineProvider::new(config, credential))
        .transpose()
}

fn pipeline_provider_config(
    endpoint: Option<String>,
    profile: Option<String>,
    model: Option<String>,
    credential: Option<String>,
) -> tect_domain::Result<Option<(JevPipelineConfig, String)>> {
    if endpoint.is_none() && profile.is_none() && model.is_none() {
        return Ok(None);
    }
    let (Some(endpoint), Some(profile), Some(model), Some(credential)) =
        (endpoint, profile, model, credential)
    else {
        return Err(Error::InvalidConfiguration);
    };
    if credential.is_empty()
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
    let config = JevPipelineConfig {
        identity: PipelineProviderIdentity {
            provider: profile,
            model,
            destination: endpoint.as_str().into(),
            wire_version: WIRE_VERSION.into(),
        },
        endpoint,
        timeout: Duration::from_secs(10),
        maximum_request_bytes: MAX_REQUEST_BYTES,
        maximum_response_bytes: MAX_RESPONSE_BYTES,
    };
    Ok(Some((config, credential)))
}

async fn shutdown_signal() -> tect_domain::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut interrupt = signal(SignalKind::interrupt()).map_err(|_| Error::TransportUnavailable)?;
    let mut terminate = signal(SignalKind::terminate()).map_err(|_| Error::TransportUnavailable)?;
    tokio::select! {
        value = interrupt.recv() => value.ok_or(Error::TransportUnavailable),
        value = terminate.recv() => value.ok_or(Error::TransportUnavailable),
    }
}

fn required_env(name: &str) -> tect_domain::Result<String> {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => Ok(value),
        _ => Err(Error::InvalidConfiguration),
    }
}

fn required_absolute_socket() -> tect_domain::Result<PathBuf> {
    let path = PathBuf::from(required_env("TECT_SOCKET")?);
    if !path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(path)
}

fn validate_socket_parent(socket: &Path) -> tect_domain::Result<()> {
    let parent = socket.parent().ok_or(Error::InvalidConfiguration)?;
    let mut current = PathBuf::new();
    for component in parent.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).map_err(|_| Error::InvalidConfiguration)?;
        if metadata.file_type().is_symlink() {
            return Err(Error::InvalidConfiguration);
        }
    }
    let metadata = fs::symlink_metadata(parent).map_err(|_| Error::InvalidConfiguration)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() || metadata.mode() & 0o7777 != 0o700
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

fn reject_existing_path(socket: &Path) -> tect_domain::Result<()> {
    match fs::symlink_metadata(socket) {
        Ok(_) => Err(Error::InvalidConfiguration),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Error::InvalidConfiguration),
    }
}

struct SocketGuard {
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl SocketGuard {
    fn capture(path: PathBuf) -> tect_domain::Result<Self> {
        let metadata = fs::symlink_metadata(&path).map_err(|_| Error::InvalidConfiguration)?;
        if !metadata.file_type().is_socket() {
            return Err(Error::InvalidConfiguration);
        }
        Ok(Self {
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    fn set_private(&self) -> tect_domain::Result<()> {
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))
            .map_err(|_| Error::InvalidConfiguration)?;
        let metadata = fs::symlink_metadata(&self.path).map_err(|_| Error::InvalidConfiguration)?;
        if !metadata.file_type().is_socket()
            || metadata.dev() != self.device
            || metadata.ino() != self.inode
            || metadata.mode() & 0o7777 != 0o600
        {
            return Err(Error::InvalidConfiguration);
        }
        Ok(())
    }

    fn remove_if_owned(&self) {
        let Ok(metadata) = fs::symlink_metadata(&self.path) else {
            return;
        };
        if metadata.file_type().is_socket()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        self.remove_if_owned();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_application::PipelineRecommendationProvider;

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
}
