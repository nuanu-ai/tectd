use crate::{Error, MAX_SOURCE_PATH_BYTES, Result};
use serde::{Deserialize, Serialize};

/// Physical directory identity captured by the host, never a permanent authority grant.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupDirectory {
    pub path: String,
    pub device: i64,
    pub inode: i64,
}

pub fn validate_setup_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > MAX_SOURCE_PATH_BYTES
        || !path.starts_with('/')
        || path.contains('\0')
        || path != "/"
            && path[1..]
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

/// This lexical check precedes any filesystem access; the host then pins physical identity.
pub fn setup_path_is_granted(path: &str, roots: &[String]) -> bool {
    validate_setup_path(path).is_ok()
        && roots.iter().any(|root| {
            validate_setup_path(root).is_ok()
                && (root == "/"
                    || path == root
                    || path
                        .strip_prefix(root)
                        .is_some_and(|rest| rest.starts_with('/')))
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupFileStatus {
    ContextUnknown,
    Missing,
    Existing,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileObservation {
    pub status: SetupFileStatus,
    pub byte_length: Option<u64>,
    pub sha256: Option<String>,
    pub reason: Option<String>,
}

impl FileObservation {
    pub fn missing() -> Self {
        Self {
            status: SetupFileStatus::Missing,
            byte_length: None,
            sha256: None,
            reason: None,
        }
    }
    pub fn unknown() -> Self {
        Self {
            status: SetupFileStatus::ContextUnknown,
            byte_length: None,
            sha256: None,
            reason: Some("task_launch_directory_not_supplied".into()),
        }
    }
    pub fn unavailable(reason: &str) -> Self {
        Self {
            status: SetupFileStatus::Unavailable,
            byte_length: None,
            sha256: None,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationOutcome {
    Created,
    AlreadyMatches,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilePublication {
    pub outcome: PublicationOutcome,
    pub sha256: String,
    pub byte_length: u64,
}
