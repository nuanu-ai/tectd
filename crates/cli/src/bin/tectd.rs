use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tect_application::{
    FixedPipelineCompatibilityPolicy, MAX_PREPARED_MATRIX_BODY_BYTES, MatrixProviderIdentity,
    PipelineProviderIdentity, SignedMatrixBudgetPreflight, SignedScopeBudgetPreflight,
    WorkspaceService,
};
use tect_domain::{
    AdvisoryModelConfiguration, AdvisoryProviderProfileRef, Error,
    PIPELINE_RECOMMENDATION_CATALOGUE_REVISION, PipelineCompatibilityPolicy,
};
use tect_host::jev_matrix_advice::{
    native_provider::{
        JevNativeMatrixConfig, JevNativeMatrixProvider, MAX_NATIVE_MATRIX_RESPONSE_BYTES,
    },
    native_wire::NATIVE_MATRIX_WIRE_VERSION,
};
use tect_host::jev_pipeline_recommendation::{
    JevPipelineConfig, JevPipelineProvider, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, WIRE_VERSION,
};
use tect_postgres::{
    BudgetOwnerKeys, PgScopeAuthoredManifestSupplier, PgScopeAuthorityObserver, PgStore,
};
use tokio::net::UnixListener;

#[path = "../daemon_config.rs"]
mod daemon_config;
#[path = "../knowledge_search_worker.rs"]
mod knowledge_search_worker;
#[path = "../model_route_daemon_config.rs"]
mod model_route_daemon_config;
use daemon_config::{
    anti_bloat_provider_from_env, database_max_connections, scope_provider_from_env,
};

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
    let matrix_provider = matrix_provider_from_env()?;
    let pipeline_compatibility_policy = pipeline_compatibility_policy_from_env()?;
    let scope_provider = scope_provider_from_env()?;
    let anti_bloat_provider = anti_bloat_provider_from_env()?;
    let model_route_provider = model_route_daemon_config::model_route_provider_from_env()?;
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
    let store = Arc::new(store);
    let inspector = Arc::new(tect_host::GitSourceInspector);
    let setup_files = Arc::new(tect_host::LocalSetupFiles);
    let mut service = if let Some(provider) = scope_provider {
        WorkspaceService::new_with_scope_advisory_adapters(
            store,
            inspector,
            setup_files,
            authority,
            supplier,
            Arc::new(SignedScopeBudgetPreflight),
            Arc::new(provider),
        )
    } else {
        WorkspaceService::new_with_scope_sources(store, inspector, setup_files, authority, supplier)
    }
    .with_pipeline_recommendation_definitions(Arc::new(
        tect_host::StaticPipelineRecommendationDefinitions,
    ));
    if let Some(provider) = pipeline_provider {
        service = service.with_pipeline_recommendation_provider(Arc::new(provider));
    }
    if let Some(provider) = anti_bloat_provider {
        service = service.with_anti_bloat_provider(Arc::new(provider));
    }
    if let Some(provider) = model_route_provider {
        service = service.with_model_route_ranking_provider(Arc::new(provider));
    }
    if let Some(provider) = matrix_provider {
        service = service.with_matrix_advisory_adapters(
            Arc::new(provider),
            Arc::new(SignedMatrixBudgetPreflight),
        );
    }
    if let Some(policy) = pipeline_compatibility_policy {
        service = service
            .with_pipeline_compatibility_policy(Arc::new(FixedPipelineCompatibilityPolicy(policy)));
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

fn pipeline_compatibility_policy_from_env()
-> tect_domain::Result<Option<PipelineCompatibilityPolicy>> {
    match std::env::var("TECT_JEV_PIPELINE_COMPATIBILITY_POLICY_JSON") {
        Ok(value) => parse_pipeline_compatibility_policy(Some(&value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(Error::InvalidConfiguration),
    }
}

fn parse_pipeline_compatibility_policy(
    json: Option<&str>,
) -> tect_domain::Result<Option<PipelineCompatibilityPolicy>> {
    json.map(|json| {
        let policy: PipelineCompatibilityPolicy =
            serde_json::from_str(json).map_err(|_| Error::InvalidConfiguration)?;
        policy.validate_host_snapshot(PIPELINE_RECOMMENDATION_CATALOGUE_REVISION)?;
        Ok(policy)
    })
    .transpose()
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

fn matrix_provider_from_env() -> tect_domain::Result<Option<JevNativeMatrixProvider>> {
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
        },
        endpoint,
        timeout: Duration::from_secs(10),
        maximum_request_bytes: MAX_PREPARED_MATRIX_BODY_BYTES,
        maximum_response_bytes: MAX_NATIVE_MATRIX_RESPONSE_BYTES,
    };
    Ok(Some((config, credential)))
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
#[path = "../tectd_tests.rs"]
mod tests;
