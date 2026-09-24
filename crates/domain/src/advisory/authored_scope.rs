use crate::{Error, Result, ScopeCandidateDraft, ScopeDecompositionKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Caller-authored alternatives are request-local until resolved against
/// the authoritative candidate snapshot and its persisted source fragments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoredScopeAlternative {
    pub key: String,
    pub kind: ScopeDecompositionKind,
    pub draft: ScopeCandidateDraft,
    pub covered_source_ref_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoredScopeSet {
    pub expected_candidate_set_revision: i64,
    pub baseline_key: String,
    pub alternatives: Vec<AuthoredScopeAlternative>,
}

impl AuthoredScopeSet {
    pub fn validate(&self) -> Result<()> {
        if self.expected_candidate_set_revision < 1
            || self.alternatives.is_empty()
            || self.alternatives.len() > 100
        {
            return Err(Error::InvalidArguments);
        }
        let mut keys = BTreeSet::new();
        for alternative in &self.alternatives {
            if !valid_request_local_key(&alternative.key)
                || !keys.insert(&alternative.key)
                || alternative.covered_source_ref_ids.iter().any(Uuid::is_nil)
                || alternative
                    .covered_source_ref_ids
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1])
            {
                return Err(Error::InvalidArguments);
            }
            alternative.draft.validate()?;
        }
        if !keys.contains(&self.baseline_key) {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

fn valid_request_local_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}
