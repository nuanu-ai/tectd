use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tect_domain::{Error, validate_native_id};
use uuid::Uuid;

pub const CLAUDE_ATTESTATION_VERSION: u32 = 1;
pub const CLAUDE_ATTESTATION_DESTINATION: &str = "tectd-mcp";
pub const CLAUDE_ATTESTATION_MAX_TTL_MS: u64 = 120_000;
pub const CLAUDE_ATTESTATION_MAX_BYTES: usize = crate::frame::MAX_FRAME_BYTES;
const MAX_TOOL_USE_ID_BYTES: usize = 1024;

/// A per-call native PreToolUse record. This is local same-user attestation,
/// not a cryptographic credential or a source of host/tenant authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaudeToolAttestation {
    pub format_version: u32,
    pub destination: String,
    pub server_alias: String,
    pub destination_host_id: Uuid,
    pub native_session_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub tool_input: Value,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
}

impl ClaudeToolAttestation {
    /// Receives the original native hook identity; never allocates an identity.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        native_session_id: String,
        tool_use_id: String,
        tool_name: String,
        tool_input: Value,
        server_alias: String,
        destination_host_id: Uuid,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self> {
        let record = Self {
            format_version: CLAUDE_ATTESTATION_VERSION,
            destination: CLAUDE_ATTESTATION_DESTINATION.to_owned(),
            server_alias,
            destination_host_id,
            native_session_id,
            tool_use_id,
            tool_name,
            tool_input,
            issued_at_unix_ms,
            expires_at_unix_ms,
        };
        record.validate_at(&record.server_alias, destination_host_id, issued_at_unix_ms)?;
        Ok(record)
    }

    pub fn validate_at(
        &self,
        server_alias: &str,
        destination_host_id: Uuid,
        now_unix_ms: u64,
    ) -> Result<()> {
        validate_server_alias(server_alias)?;
        validate_native_id(&self.native_session_id)?;
        validate_tool_use_id(&self.tool_use_id)?;
        let prefix = format!("mcp__{server_alias}__");
        let suffix = self
            .tool_name
            .strip_prefix(&prefix)
            .ok_or(Error::InvalidNativeSession)?;
        if self.format_version != CLAUDE_ATTESTATION_VERSION
            || self.destination != CLAUDE_ATTESTATION_DESTINATION
            || self.server_alias != server_alias
            || destination_host_id.is_nil()
            || self.destination_host_id != destination_host_id
            || !matches!(
                suffix,
                "get_state" | "help" | "query" | "command" | "execute"
            )
            || !self.tool_input.is_object()
            || self.issued_at_unix_ms > now_unix_ms
            || self.expires_at_unix_ms <= now_unix_ms
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || self.expires_at_unix_ms - self.issued_at_unix_ms > CLAUDE_ATTESTATION_MAX_TTL_MS
        {
            return Err(Error::InvalidNativeSession);
        }
        if serde_json::to_vec(self)
            .map_err(|_| Error::InvalidNativeSession)?
            .len()
            > CLAUDE_ATTESTATION_MAX_BYTES
        {
            return Err(Error::InvalidNativeSession);
        }
        Ok(())
    }
}

pub fn claude_attestation_filename(tool_use_id: &str) -> Result<String> {
    validate_tool_use_id(tool_use_id)?;
    Ok(format!("{:x}.json", Sha256::digest(tool_use_id.as_bytes())))
}

pub(crate) fn validate_server_alias(alias: &str) -> Result<()> {
    if alias.is_empty()
        || alias.len() > 64
        || !alias
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    {
        return Err(Error::InvalidNativeSession);
    }
    Ok(())
}

fn validate_tool_use_id(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > MAX_TOOL_USE_ID_BYTES || value.as_bytes().contains(&0) {
        return Err(Error::InvalidNativeSession);
    }
    Ok(())
}
