use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tect_application::WorkspaceService;
use tect_domain::{Error, RequestContext, Result};

const MAX_FILE_BYTES: u64 = 64 * 1024;
const MAX_CONTEXTS: usize = 16;
const MAX_BATCH: u32 = 64;
const MIN_INTERVAL_SECONDS: u64 = 5;
const MAX_INTERVAL_SECONDS: u64 = 3600;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SearchWorkerConfig {
    contexts: Vec<RequestContext>,
    batch_limit: u32,
    interval_seconds: u64,
}

impl SearchWorkerConfig {
    pub(crate) fn from_env() -> Result<Option<Self>> {
        let Some(path) = std::env::var_os("TECT_KNOWLEDGE_SEARCH_CONTEXTS") else {
            return Ok(None);
        };
        Self::load(PathBuf::from(path)).map(Some)
    }

    fn load(path: PathBuf) -> Result<Self> {
        let mut file = validate_and_open(&path)?;
        let mut bytes = Vec::new();
        file.by_ref()
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::InvalidConfiguration)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(Error::InvalidConfiguration);
        }
        let value: Self =
            serde_json::from_slice(&bytes).map_err(|_| Error::InvalidConfiguration)?;
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<()> {
        if self.contexts.is_empty()
            || self.contexts.len() > MAX_CONTEXTS
            || !(1..=MAX_BATCH).contains(&self.batch_limit)
            || !(MIN_INTERVAL_SECONDS..=MAX_INTERVAL_SECONDS).contains(&self.interval_seconds)
        {
            return Err(Error::InvalidConfiguration);
        }
        let mut identities = BTreeSet::new();
        for context in &self.contexts {
            context
                .validate()
                .map_err(|_| Error::InvalidConfiguration)?;
            if !identities.insert((
                context.auth.host_id,
                context.native_session_id.clone(),
                context.workspace_key.clone(),
            )) {
                return Err(Error::InvalidConfiguration);
            }
        }
        Ok(())
    }
}

pub(crate) fn maintenance_enabled() -> Result<bool> {
    match std::env::var("TECT_KNOWLEDGE_MAINTENANCE") {
        Err(std::env::VarError::NotPresent) => Ok(false),
        Ok(value) if value == "1" => Ok(true),
        _ => Err(Error::InvalidConfiguration),
    }
}

fn validate_and_open(path: &Path) -> Result<std::fs::File> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err(Error::InvalidConfiguration);
    }
    let path_metadata = fs::symlink_metadata(path).map_err(|_| Error::InvalidConfiguration)?;
    let file = std::fs::File::open(path).map_err(|_| Error::InvalidConfiguration)?;
    let metadata = file.metadata().map_err(|_| Error::InvalidConfiguration)?;
    if path_metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_FILE_BYTES
        || metadata.permissions().mode() & 0o7777 != 0o600
        || metadata.uid() != rustix::process::getuid().as_raw()
        || metadata.dev() != path_metadata.dev()
        || metadata.ino() != path_metadata.ino()
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(file)
}

pub(crate) async fn run(
    service: Arc<WorkspaceService>,
    config: SearchWorkerConfig,
    search_enabled: bool,
    maintenance_enabled: bool,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(config.interval_seconds));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        for context in &config.contexts {
            if maintenance_enabled {
                let _ = service
                    .process_knowledge_maintenance_tasks(context, config.batch_limit)
                    .await;
            }
            if search_enabled {
                let _ = service
                    .process_knowledge_search_jobs(context, config.batch_limit)
                    .await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::OpenOptionsExt;
    use uuid::Uuid;

    fn file(body: &str, mode: u32) -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("contexts.json");
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(mode)
            .open(&path)
            .and_then(|mut file| std::io::Write::write_all(&mut file, body.as_bytes()))
            .unwrap();
        (temp, path)
    }

    #[test]
    fn private_file_contains_only_bounded_existing_contexts() {
        let id = Uuid::new_v4();
        let body = serde_json::json!({"contexts":[{"auth":{"host_id":id,"credential":"a".repeat(64)},"native_session_id":Uuid::new_v4().to_string(),"workspace_key":"existing"}],"batch_limit":1,"interval_seconds":30}).to_string();
        let (_temp, path) = file(&body, 0o600);
        let config = SearchWorkerConfig::load(path).unwrap();
        assert_eq!(config.contexts.len(), 1);
        assert_eq!(config.batch_limit, 1);
    }

    #[test]
    fn permissions_unknown_fields_and_unbounded_batches_fail_closed() {
        let (_temp, path) = file("{}", 0o644);
        assert!(SearchWorkerConfig::load(path).is_err());
        let (_temp, path) = file(
            r#"{"contexts":[],"batch_limit":65,"interval_seconds":1,"session_factory":true}"#,
            0o600,
        );
        assert!(SearchWorkerConfig::load(path).is_err());
    }
}
