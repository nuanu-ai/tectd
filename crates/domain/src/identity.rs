use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostAuth {
    pub host_id: Uuid,
    pub credential: String,
}

impl fmt::Debug for HostAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostAuth")
            .field("host_id", &self.host_id)
            .field("credential", &"[redacted]")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestContext {
    pub auth: HostAuth,
    pub native_session_id: String,
    pub workspace_key: String,
}

impl RequestContext {
    pub fn validate(&self) -> Result<()> {
        validate_native_id(&self.native_session_id)?;
        validate_workspace_key(&self.workspace_key)?;
        if self.auth.host_id.is_nil()
            || self.auth.credential.len() != 64
            || !self.auth.credential.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostIdentity {
    pub host_id: Uuid,
    pub tenant_id: Uuid,
    pub principal_id: Uuid,
    pub allowed_source_roots: Vec<String>,
    pub allowed_setup_roots: Vec<String>,
}

pub fn validate_native_id(value: &str) -> Result<()> {
    let id = Uuid::parse_str(value).map_err(|_| Error::InvalidNativeSession)?;
    if id.is_nil() || id.to_string() != value {
        return Err(Error::InvalidNativeSession);
    }
    Ok(())
}

pub fn validate_workspace_key(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(Error::InvalidWorkspaceKey);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_never_accepts_paths_or_generated_fallbacks() {
        for value in ["", "/__tect_test__/workspace", "../project", "a/b", " key"] {
            assert_eq!(
                validate_workspace_key(value),
                Err(Error::InvalidWorkspaceKey)
            );
        }
        for value in [
            "",
            "process:123",
            "random",
            "00000000-0000-0000-0000-000000000000",
        ] {
            assert_eq!(validate_native_id(value), Err(Error::InvalidNativeSession));
        }
    }
}
