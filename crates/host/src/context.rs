use crate::Result;
use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use tect_domain::{Error, HostAuth, RequestContext, validate_native_id, validate_workspace_key};

const MAX_CONFIG_BYTES: u64 = 4 * 1024;

pub fn context_from_env() -> Result<RequestContext> {
    let config_path = required_env("TECT_HOST_CONFIG")?;
    let workspace_key = required_env("TECT_WORKSPACE_KEY")?;
    validate_workspace_key(&workspace_key)?;

    let codex_session = optional_env("CODEX_SESSION_ID")?;
    let codex_thread = optional_env("CODEX_THREAD_ID")?;
    let native_session_id =
        native_session_from_values(codex_session.as_deref(), codex_thread.as_deref())?;
    let auth = read_host_auth(Path::new(&config_path))?;

    let context = RequestContext {
        auth,
        native_session_id,
        workspace_key,
    };
    context.validate()?;
    Ok(context)
}

fn required_env(name: &str) -> Result<String> {
    match env::var(name) {
        Ok(value) if !value.is_empty() => Ok(value),
        _ => Err(Error::InvalidConfiguration),
    }
}

fn optional_env(name: &str) -> Result<Option<String>> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(Error::InvalidNativeSession),
    }
}

fn native_session_from_values(session: Option<&str>, thread: Option<&str>) -> Result<String> {
    let value = match (session, thread) {
        (Some(session), Some(thread)) if session == thread => session,
        (Some(_), Some(_)) => return Err(Error::InvalidNativeSession),
        (Some(session), None) => session,
        (None, Some(thread)) => thread,
        (None, None) => return Err(Error::InvalidNativeSession),
    };
    validate_native_id(value)?;
    Ok(value.to_owned())
}

fn read_host_auth(path: &Path) -> Result<HostAuth> {
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

    const A: &str = "019d14d7-4678-7ee1-8000-000000000001";
    const B: &str = "019d14d7-4678-7ee1-8000-000000000002";

    #[test]
    fn native_session_selection_is_explicit_and_unambiguous() {
        assert_eq!(native_session_from_values(Some(A), None).unwrap(), A);
        assert_eq!(native_session_from_values(None, Some(A)).unwrap(), A);
        assert_eq!(native_session_from_values(Some(A), Some(A)).unwrap(), A);
        assert_eq!(
            native_session_from_values(Some(A), Some(B)),
            Err(Error::InvalidNativeSession)
        );
        assert_eq!(
            native_session_from_values(None, None),
            Err(Error::InvalidNativeSession)
        );
    }

    #[test]
    fn native_session_selection_rejects_empty_and_noncanonical_ids() {
        for value in ["", "019D14D7-4678-7EE1-8000-000000000001", "process:1"] {
            assert_eq!(
                native_session_from_values(Some(value), None),
                Err(Error::InvalidNativeSession)
            );
        }
    }
}
