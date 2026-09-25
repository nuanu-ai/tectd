use std::collections::HashMap;
use tect_domain::{AdvisoryBudgetPolicy, Error, Result};
use uuid::Uuid;

use crate::verify_budget_policy_approval;

/// Immutable runtime trust anchors for budget approvals. An empty registry denies all sends.
#[derive(Clone, Default)]
pub struct BudgetOwnerKeys {
    keys: HashMap<(Uuid, Uuid), [u8; 32]>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    workspace_id: Uuid,
    owner_id: Uuid,
    public_key_hex: String,
}

impl BudgetOwnerKeys {
    /// Parse the explicit daemon configuration; duplicate identities are invalid.
    pub fn from_json(value: &str) -> Result<Self> {
        let entries: Vec<Entry> =
            serde_json::from_str(value).map_err(|_| Error::InvalidConfiguration)?;
        let mut keys = HashMap::with_capacity(entries.len());
        for entry in entries {
            if entry.workspace_id.is_nil()
                || entry.owner_id.is_nil()
                || entry.public_key_hex.len() != 64
                || !entry.public_key_hex.is_ascii()
            {
                return Err(Error::InvalidConfiguration);
            }
            let mut public_key = [0u8; 32];
            for (index, byte) in public_key.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&entry.public_key_hex[index * 2..index * 2 + 2], 16)
                    .map_err(|_| Error::InvalidConfiguration)?;
            }
            if keys
                .insert((entry.workspace_id, entry.owner_id), public_key)
                .is_some()
            {
                return Err(Error::InvalidConfiguration);
            }
        }
        Ok(Self { keys })
    }

    pub(crate) fn authorizes(&self, workspace_id: Uuid, policy: &AdvisoryBudgetPolicy) -> bool {
        self.keys
            .get(&(workspace_id, policy.approved_by()))
            .is_some_and(|key| {
                verify_budget_policy_approval(workspace_id, policy.approved_by(), policy, key)
                    .is_ok()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_is_strict_and_defaults_to_deny() {
        assert!(BudgetOwnerKeys::default().keys.is_empty());
        let workspace = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let row = format!(
            r#"{{"workspace_id":"{workspace}","owner_id":"{owner}","public_key_hex":"{}"}}"#,
            "00".repeat(32)
        );
        assert!(BudgetOwnerKeys::from_json(&format!("[{row}]")).is_ok());
        for invalid in [
            "not json".to_owned(),
            "{}".to_owned(),
            format!("[{row},{row}]"),
            format!("[{}]", row.replace(&"00".repeat(32), "gg")),
            format!("[{}]", row.replace(&"00".repeat(32), &"é".repeat(32))),
            format!("[{}]", row.replace(&"00".repeat(32), &"00".repeat(31))),
            format!("[{}]", row.replace("public_key_hex", "private_key_hex")),
        ] {
            assert!(BudgetOwnerKeys::from_json(&invalid).is_err(), "{invalid}");
        }
    }
}
