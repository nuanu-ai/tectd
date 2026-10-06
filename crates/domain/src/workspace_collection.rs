use crate::{CandidateSetSummary, Error, NativePlanningSummary, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceCollection {
    CandidateSets,
    NativePlanning,
}

impl WorkspaceCollection {
    fn name(self) -> &'static str {
        match self {
            Self::CandidateSets => "candidate_sets",
            Self::NativePlanning => "native_planning",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct WorkspaceCollectionCursor {
    pub workspace_id: Uuid,
    pub collection: WorkspaceCollection,
    pub anchor_id: Uuid,
}

impl WorkspaceCollectionCursor {
    pub fn parse(value: &str) -> Result<Self> {
        let mut parts = value.split('/');
        let workspace = parts.next().and_then(|s| s.strip_prefix("w:"));
        let collection = parts.next().and_then(|s| s.strip_prefix("c:"));
        let anchor = parts.next().and_then(|s| s.strip_prefix("r:"));
        if parts.next().is_some() {
            return Err(Error::InvalidArguments);
        }
        let workspace_id = Uuid::parse_str(workspace.ok_or(Error::InvalidArguments)?)
            .map_err(|_| Error::InvalidArguments)?;
        let anchor_id = Uuid::parse_str(anchor.ok_or(Error::InvalidArguments)?)
            .map_err(|_| Error::InvalidArguments)?;
        let collection = match collection {
            Some("candidate_sets") => WorkspaceCollection::CandidateSets,
            Some("native_planning") => WorkspaceCollection::NativePlanning,
            _ => return Err(Error::InvalidArguments),
        };
        let cursor = Self {
            workspace_id,
            collection,
            anchor_id,
        };
        if workspace_id.is_nil() || anchor_id.is_nil() || cursor.encode() != value {
            return Err(Error::InvalidArguments);
        }
        Ok(cursor)
    }

    pub fn encode(self) -> String {
        format!(
            "w:{}/c:{}/r:{}",
            self.workspace_id,
            self.collection.name(),
            self.anchor_id
        )
    }

    pub fn validate(self, workspace_id: Uuid, collection: WorkspaceCollection) -> Result<()> {
        if self.workspace_id != workspace_id
            || self.collection != collection
            || self.anchor_id.is_nil()
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

impl TryFrom<String> for WorkspaceCollectionCursor {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        Self::parse(&value)
    }
}
impl From<WorkspaceCollectionCursor> for String {
    fn from(value: WorkspaceCollectionCursor) -> Self {
        value.encode()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSetList {
    pub candidate_sets: Vec<CandidateSetSummary>,
    pub next_after: Option<WorkspaceCollectionCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativePlanningList {
    pub native_planning: Vec<NativePlanningSummary>,
    pub next_after: Option<WorkspaceCollectionCursor>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_cursors_bind_collection_and_workspace() {
        let cursor = WorkspaceCollectionCursor {
            workspace_id: Uuid::from_u128(1),
            collection: WorkspaceCollection::CandidateSets,
            anchor_id: Uuid::from_u128(2),
        };
        assert_eq!(
            WorkspaceCollectionCursor::parse(&cursor.encode()).unwrap(),
            cursor
        );
        assert!(
            cursor
                .validate(Uuid::from_u128(3), WorkspaceCollection::CandidateSets)
                .is_err()
        );
        assert!(
            cursor
                .validate(cursor.workspace_id, WorkspaceCollection::NativePlanning)
                .is_err()
        );
        assert!(WorkspaceCollectionCursor::parse(&format!("{}/extra", cursor.encode())).is_err());
        assert_eq!(
            serde_json::to_value(cursor).unwrap(),
            serde_json::json!(cursor.encode())
        );
    }
}
