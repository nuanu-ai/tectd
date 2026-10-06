//! Optional host-owned capability evidence, loaded once and kept independent
//! of caller Work facts and the allowed-route catalogue.
use serde::Deserialize;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use tect_application::ModelRouteHostCapabilitiesProvider;
use tect_domain::{Error, ModelRouteFact, ModelRouteHostCapabilities, Result};

const MAX_FILE_BYTES: u64 = 64 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotFile {
    schema: String,
    version: u64,
    digest: String,
    capabilities: Vec<String>,
}

/// An explicit, immutable host assertion. An empty vector is Known, not Unknown.
pub struct StaticModelRouteHostCapabilities(ModelRouteHostCapabilities);

impl StaticModelRouteHostCapabilities {
    /// Private absolute JSON path; absence leaves capabilities Unknown.
    pub fn from_env() -> Result<Option<Self>> {
        let Some(path) = std::env::var_os("TECT_MODEL_ROUTE_HOST_CAPABILITIES") else {
            return Ok(None);
        };
        Self::load_optional(Some(Path::new(&path)))
    }

    pub fn load_optional(path: Option<&Path>) -> Result<Option<Self>> {
        path.map(Self::load).transpose()
    }

    fn load(path: &Path) -> Result<Self> {
        let mut file: File = super::model_route_catalogue::validate_and_open(path)?;
        let mut bytes = Vec::new();
        file.by_ref()
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::InvalidConfiguration)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(Error::InvalidConfiguration);
        }
        let source: SnapshotFile =
            serde_json::from_slice(&bytes).map_err(|_| Error::InvalidConfiguration)?;
        let snapshot = ModelRouteHostCapabilities {
            schema: source.schema,
            version: source.version,
            capabilities: source.capabilities,
        };
        if snapshot.digest().map_err(|_| Error::InvalidConfiguration)? != source.digest {
            return Err(Error::InvalidConfiguration);
        }
        Ok(Self(snapshot))
    }
}

impl ModelRouteHostCapabilitiesProvider for StaticModelRouteHostCapabilities {
    fn host_capabilities(&self) -> Result<ModelRouteFact<Vec<String>>> {
        self.0.fact()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use tect_domain::{MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA, ModelRouteFactProvenance};

    fn write(body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("capabilities.json");
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

    fn valid_json(capabilities: &[&str]) -> String {
        let snapshot = ModelRouteHostCapabilities {
            schema: MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
            version: 1,
            capabilities: capabilities.iter().map(|v| (*v).into()).collect(),
        };
        serde_json::json!({
            "schema": snapshot.schema,
            "version": snapshot.version,
            "digest": snapshot.digest().unwrap(),
            "capabilities": snapshot.capabilities,
        })
        .to_string()
    }

    #[test]
    fn absent_is_unknown_and_explicit_empty_is_known_host_evidence() {
        assert!(
            StaticModelRouteHostCapabilities::load_optional(None)
                .unwrap()
                .is_none()
        );
        let (_temp, path) = write(&valid_json(&[]));
        let provider = StaticModelRouteHostCapabilities::load_optional(Some(&path))
            .unwrap()
            .unwrap();
        let ModelRouteFact::Known {
            value,
            provenance: ModelRouteFactProvenance::Host { evidence_ref },
        } = provider.host_capabilities().unwrap()
        else {
            panic!("known host fact")
        };
        assert!(value.is_empty());
        assert!(evidence_ref.contains("tect.model-route-host-capabilities/1:v1:"));
        assert!(evidence_ref.ends_with(&provider.0.digest().unwrap()));
    }

    #[test]
    fn snapshot_is_fixed_and_invalid_or_untrusted_files_fail_closed() {
        let (_temp, path) = write(&valid_json(&["model-api"]));
        let provider = StaticModelRouteHostCapabilities::load_optional(Some(&path))
            .unwrap()
            .unwrap();
        std::fs::write(&path, "broken").unwrap();
        assert!(matches!(
            provider.host_capabilities().unwrap(),
            ModelRouteFact::Known { .. }
        ));
        assert!(StaticModelRouteHostCapabilities::load_optional(Some(&path)).is_err());
        for bad in [
            "{".to_owned(),
            valid_json(&[]).replace("\"digest\":\"", "\"digest\":\"0"),
            valid_json(&[]).replace("\"version\":1", "\"version\":0"),
            valid_json(&[]).replace("\"capabilities\":[]", "\"capabilities\":[\"x\",\"x\"]"),
            valid_json(&[]).replace("\"version\":1", "\"version\":1,\"extra\":true"),
        ] {
            let (_temp, path) = write(&bad);
            assert!(StaticModelRouteHostCapabilities::load_optional(Some(&path)).is_err());
        }
        let (_temp, path) = write(&valid_json(&[]));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(StaticModelRouteHostCapabilities::load_optional(Some(&path)).is_err());
        let (_temp, target) = write(&valid_json(&[]));
        let link = target.with_extension("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(StaticModelRouteHostCapabilities::load_optional(Some(&link)).is_err());
    }
}
