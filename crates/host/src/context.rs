use crate::Result;
use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use tect_domain::{Error, HostAuth, RequestContext, validate_native_id, validate_workspace_key};

const MAX_CONFIG_BYTES: u64 = 4 * 1024;

#[derive(Clone)]
pub struct HostContext {
    auth: HostAuth,
    workspace_key: String,
}

impl HostContext {
    pub(crate) fn new(auth: HostAuth, workspace_key: String) -> Result<Self> {
        validate_workspace_key(&workspace_key)?;
        Ok(Self {
            auth,
            workspace_key,
        })
    }

    pub(crate) fn request_context(&self, native_session_id: &str) -> Result<RequestContext> {
        validate_native_id(native_session_id)?;
        let context = RequestContext {
            auth: self.auth.clone(),
            native_session_id: native_session_id.to_owned(),
            workspace_key: self.workspace_key.clone(),
        };
        context.validate()?;
        Ok(context)
    }
}

pub fn host_context_from_env() -> Result<HostContext> {
    let config_path = required_env("TECT_HOST_CONFIG")?;
    let workspace_key = required_env("TECT_WORKSPACE_KEY")?;
    let auth = read_host_auth_file(Path::new(&config_path))?;
    HostContext::new(auth, workspace_key)
}

fn required_env(name: &str) -> Result<String> {
    match env::var(name) {
        Ok(value) if !value.is_empty() => Ok(value),
        _ => Err(Error::InvalidConfiguration),
    }
}

/// Read an existing host credential without following symlinks or accepting a
/// non-private, oversized, or replaced file.
pub fn read_host_auth_file(path: &Path) -> Result<HostAuth> {
    if !path.is_absolute() {
        return Err(Error::InvalidConfiguration);
    }
    reject_non_normal_or_symlinked_path(path)?;

    let mut file = File::open(path).map_err(|_| Error::InvalidConfiguration)?;
    let opened = file.metadata().map_err(|_| Error::InvalidConfiguration)?;
    validate_config_metadata(&opened)?;

    let mut bytes = Vec::with_capacity(opened.len() as usize);
    file.by_ref()
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::InvalidConfiguration)?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(Error::InvalidConfiguration);
    }

    let current = fs::symlink_metadata(path).map_err(|_| Error::InvalidConfiguration)?;
    validate_config_metadata(&current)?;
    if current.dev() != opened.dev() || current.ino() != opened.ino() {
        return Err(Error::InvalidConfiguration);
    }

    let auth: HostAuth = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidConfiguration)?;
    if auth.host_id.is_nil()
        || auth.credential.len() != 64
        || !auth.credential.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(auth)
}

fn reject_non_normal_or_symlinked_path(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::Normal(_) => current.push(component.as_os_str()),
            _ => return Err(Error::InvalidConfiguration),
        }
        let metadata = fs::symlink_metadata(&current).map_err(|_| Error::InvalidConfiguration)?;
        if metadata.file_type().is_symlink() {
            return Err(Error::InvalidConfiguration);
        }
    }
    Ok(())
}

fn validate_config_metadata(metadata: &fs::Metadata) -> Result<()> {
    if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES || metadata.mode() & 0o7777 != 0o600
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use uuid::Uuid;

    fn write_auth(path: &Path, auth: &HostAuth) {
        fs::write(path, serde_json::to_vec(auth).unwrap()).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn existing_auth_file_is_bounded_private_and_not_symlinked() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let auth = HostAuth {
            host_id: Uuid::new_v4(),
            credential: "a".repeat(64),
        };
        let path = root.join("host.json");
        write_auth(&path, &auth);
        let loaded = read_host_auth_file(&path).unwrap();
        assert_eq!(loaded.host_id, auth.host_id);
        assert_eq!(loaded.credential, auth.credential);

        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        assert!(matches!(
            read_host_auth_file(&path),
            Err(Error::InvalidConfiguration)
        ));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

        let link = root.join("host-link.json");
        symlink(&path, &link).unwrap();
        assert!(matches!(
            read_host_auth_file(&link),
            Err(Error::InvalidConfiguration)
        ));

        let invalid = root.join("invalid.json");
        write_auth(
            &invalid,
            &HostAuth {
                host_id: Uuid::nil(),
                credential: "not-a-credential".into(),
            },
        );
        assert!(matches!(
            read_host_auth_file(&invalid),
            Err(Error::InvalidConfiguration)
        ));

        let oversized = root.join("oversized.json");
        fs::write(&oversized, vec![b'x'; (MAX_CONFIG_BYTES + 1) as usize]).unwrap();
        fs::set_permissions(&oversized, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(
            read_host_auth_file(&oversized),
            Err(Error::InvalidConfiguration)
        ));
    }
}
