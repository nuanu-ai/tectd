//! Optional, host-owned route catalogue snapshot loaded once at daemon startup.
use serde::Deserialize;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path};
use tect_application::ModelRouteCatalogueProvider;
use tect_domain::{Error, ModelRoute, ModelRouteCatalogue, Result};

const MAX_FILE_BYTES: u64 = 64 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotFile {
    schema: String,
    version: u64,
    digest: String,
    routes: Vec<ModelRoute>,
}

/// A fixed snapshot: subsequent file changes cannot alter a running daemon's policy.
pub struct StaticModelRouteCatalogue(ModelRouteCatalogue);

impl StaticModelRouteCatalogue {
    /// `TECT_MODEL_ROUTE_CATALOGUE` names an absolute, private JSON snapshot.
    pub fn from_env() -> Result<Option<Self>> {
        let Some(path) = std::env::var_os("TECT_MODEL_ROUTE_CATALOGUE") else {
            return Ok(None);
        };
        Self::load_optional(Some(Path::new(&path)))
    }

    pub fn load_optional(path: Option<&Path>) -> Result<Option<Self>> {
        path.map(Self::load).transpose()
    }

    fn load(path: &Path) -> Result<Self> {
        let mut file = validate_and_open(path)?;
        let mut bytes = Vec::new();
        file.by_ref()
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::InvalidConfiguration)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(Error::InvalidConfiguration);
        }
        let snapshot: SnapshotFile =
            serde_json::from_slice(&bytes).map_err(|_| Error::InvalidConfiguration)?;
        let catalogue = ModelRouteCatalogue {
            schema: snapshot.schema,
            version: snapshot.version,
            routes: snapshot.routes,
        };
        let actual = catalogue
            .digest()
            .map_err(|_| Error::InvalidConfiguration)?;
        if snapshot.digest != actual {
            return Err(Error::InvalidConfiguration);
        }
        Ok(Self(catalogue))
    }
}

impl ModelRouteCatalogueProvider for StaticModelRouteCatalogue {
    fn catalogue(&self) -> Result<Option<ModelRouteCatalogue>> {
        Ok(Some(self.0.clone()))
    }
}

pub(crate) fn validate_and_open(path: &Path) -> Result<File> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err(Error::InvalidConfiguration);
    }
    let path_metadata = fs::symlink_metadata(path).map_err(|_| Error::InvalidConfiguration)?;
    let file = File::open(path).map_err(|_| Error::InvalidConfiguration)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::OpenOptionsExt;

    fn route() -> ModelRoute {
        ModelRoute {
            id: "route-a".into(),
            provider: "provider-a".into(),
            model: "model-a".into(),
            effort: "medium".into(),
            enabled: true,
            allowed_matrix_choice_ids: vec!["choice-a".into()],
            allowed_roles: vec!["agent".into()],
            allowed_tools: vec!["code".into()],
            allowed_data_classes: vec!["internal".into()],
            required_host_capabilities: vec![],
            minimum_budget_units: 0,
            minimum_latency_ms: 0,
        }
    }

    fn write(body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("routes.json");
        use std::io::Write;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
        (temp, path)
    }

    fn valid_json(routes: &[ModelRoute]) -> String {
        let catalogue = ModelRouteCatalogue {
            schema: tect_domain::MODEL_ROUTE_CATALOGUE_SCHEMA.into(),
            version: 1,
            routes: routes.to_vec(),
        };
        serde_json::json!({
            "schema": catalogue.schema,
            "version": catalogue.version,
            "digest": catalogue.digest().unwrap(),
            "routes": routes.iter().map(|route| serde_json::json!({
                "id": route.id,
                "provider": route.provider,
                "model": route.model,
                "effort": route.effort,
                "enabled": route.enabled,
                "allowed_matrix_choice_ids": route.allowed_matrix_choice_ids,
                "allowed_roles": route.allowed_roles,
                "allowed_tools": route.allowed_tools,
                "allowed_data_classes": route.allowed_data_classes,
                "required_host_capabilities": route.required_host_capabilities,
                "minimum_budget_units": route.minimum_budget_units,
                "minimum_latency_ms": route.minimum_latency_ms,
            })).collect::<Vec<_>>()
        })
        .to_string()
    }

    #[test]
    fn absent_config_is_unavailable() {
        assert!(
            StaticModelRouteCatalogue::load_optional(None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn valid_snapshot_is_fixed_and_digested() {
        let body = valid_json(&[route()]);
        let (_temp, path) = write(&body);
        let provider = StaticModelRouteCatalogue::load_optional(Some(&path))
            .unwrap()
            .unwrap();
        assert_eq!(
            provider.catalogue().unwrap().unwrap().routes[0].id,
            "route-a"
        );
        fs::write(&path, "broken").unwrap();
        assert_eq!(provider.catalogue().unwrap().unwrap().version, 1);
    }

    #[test]
    fn malformed_schema_digest_and_unknown_fields_fail_closed() {
        let body = valid_json(&[route()]);
        for malformed in [
            "{".to_string(),
            body.replace("tect.model-routes/1", "tect.model-routes/2"),
            body.replace("\"digest\":\"", "\"digest\":\"0"),
            body.replace("\"version\":1", "\"version\":0"),
            body.replace("\"version\":1", "\"version\":1,\"extra\":true"),
        ] {
            let (_temp, path) = write(&malformed);
            assert!(StaticModelRouteCatalogue::load_optional(Some(&path)).is_err());
        }
    }

    #[test]
    fn duplicate_routes_fail_closed() {
        let body = valid_json(&[route()]);
        let mut value: serde_json::Value = serde_json::from_str(&body).unwrap();
        let duplicate = value["routes"][0].clone();
        value["routes"].as_array_mut().unwrap().push(duplicate);
        let (_temp, path) = write(&value.to_string());
        assert!(StaticModelRouteCatalogue::load_optional(Some(&path)).is_err());
    }
}
