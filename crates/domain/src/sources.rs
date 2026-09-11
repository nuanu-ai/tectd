use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

pub const MAX_WORKTREES: usize = 100;
pub const MAX_SOURCE_PATH_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    pub common_dir: String,
    pub worktree_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredSource {
    pub id: Uuid,
    pub repository_id: Uuid,
    pub path: String,
    pub common_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePage {
    pub items: Vec<RegisteredSource>,
    pub next_after: Option<Uuid>,
}

pub fn validate_selection(ids: &[Uuid]) -> Result<()> {
    if ids.len() > MAX_WORKTREES
        || ids.iter().any(Uuid::is_nil)
        || ids.iter().copied().collect::<HashSet<_>>().len() != ids.len()
    {
        return Err(Error::InvalidWorktreeSelection);
    }
    Ok(())
}
