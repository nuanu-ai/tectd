//! Explicit native identity providers. Neither environment nor process IDs
//! supply a native session ID. Claude binds each RPC to its original hook call.
mod record;
mod secure_file;

use crate::Result;
pub use record::{
    CLAUDE_ATTESTATION_DESTINATION, CLAUDE_ATTESTATION_MAX_BYTES, CLAUDE_ATTESTATION_MAX_TTL_MS,
    CLAUDE_ATTESTATION_VERSION, ClaudeToolAttestation, claude_attestation_filename,
};
pub use secure_file::open_claude_attestation_directory;
use serde_json::{Map, Value};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tect_domain::{Error, validate_native_id};
use uuid::Uuid;

#[derive(Clone)]
pub struct ClaudeIdentityConfig {
    pub(crate) directory: PathBuf,
    pub(crate) server_alias: String,
    pub(crate) destination_host_id: Uuid,
}

impl ClaudeIdentityConfig {
    pub fn new(
        directory: PathBuf,
        server_alias: String,
        destination_host_id: Uuid,
    ) -> Result<Self> {
        record::validate_server_alias(&server_alias)?;
        if destination_host_id.is_nil() {
            return Err(Error::InvalidNativeSession);
        }
        // Fail early for an unsafe/missing holder; revalidate on every call.
        let _ = open_claude_attestation_directory(&directory)?;
        Ok(Self {
            directory,
            server_alias,
            destination_host_id,
        })
    }
}

#[derive(Clone, Default)]
pub(crate) enum NativeIdentityProvider {
    #[default]
    CodexMetadata,
    ClaudePreToolUse(ClaudeIdentityConfig),
}

impl NativeIdentityProvider {
    pub(crate) fn resolve(
        &self,
        metadata: Option<&Map<String, Value>>,
        tool: &str,
        arguments: &Value,
        arguments_present: bool,
    ) -> Result<String> {
        self.resolve_with_clock(metadata, tool, arguments, arguments_present, unix_time_ms)
    }

    #[cfg(test)]
    pub(super) fn resolve_at(
        &self,
        metadata: Option<&Map<String, Value>>,
        tool: &str,
        arguments: &Value,
        arguments_present: bool,
        now: u64,
    ) -> Result<String> {
        self.resolve_with_clock(metadata, tool, arguments, arguments_present, || Ok(now))
    }

    fn resolve_with_clock(
        &self,
        metadata: Option<&Map<String, Value>>,
        tool: &str,
        arguments: &Value,
        arguments_present: bool,
        now: impl FnOnce() -> Result<u64>,
    ) -> Result<String> {
        let metadata = metadata.ok_or(Error::InvalidNativeSession)?;
        match self {
            Self::CodexMetadata => {
                let id = metadata
                    .get("threadId")
                    .and_then(Value::as_str)
                    .ok_or(Error::InvalidNativeSession)?;
                validate_native_id(id)?;
                Ok(id.to_owned())
            }
            Self::ClaudePreToolUse(config) => {
                let call_id = metadata
                    .get("claudecode/toolUseId")
                    .and_then(Value::as_str)
                    .ok_or(Error::InvalidNativeSession)?;
                if !arguments_present {
                    return Err(Error::InvalidNativeSession);
                }
                let bytes = secure_file::read_record(&config.directory, call_id)?;
                let record: ClaudeToolAttestation =
                    serde_json::from_slice(&bytes).map_err(|_| Error::InvalidNativeSession)?;
                record.validate_at(&config.server_alias, config.destination_host_id, now()?)?;
                if record.tool_use_id != call_id
                    || record.tool_name != format!("mcp__{}__{tool}", config.server_alias)
                    || record.tool_input != *arguments
                {
                    return Err(Error::InvalidNativeSession);
                }
                if let Some(thread_id) = metadata.get("threadId") {
                    let id = thread_id.as_str().ok_or(Error::InvalidNativeSession)?;
                    validate_native_id(id)?;
                    if id != record.native_session_id {
                        return Err(Error::InvalidNativeSession);
                    }
                }
                Ok(record.native_session_id)
            }
        }
    }
}

pub fn unix_time_ms() -> Result<u64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::InvalidNativeSession)?;
    u64::try_from(elapsed.as_millis()).map_err(|_| Error::InvalidNativeSession)
}

#[cfg(test)]
mod tests;
