//! Versioned Jev dispatch limits. No deployment defaults live here.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdvisoryBudgetCeilings {
    pub provider_calls: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub request_utf8_bytes: i64,
    pub elapsed_monotonic_ms: i64,
    pub retry_dispatches: i64,
}

impl AdvisoryBudgetCeilings {
    pub fn validate(self) -> Result<()> {
        if [
            self.provider_calls,
            self.input_tokens,
            self.output_tokens,
            self.request_utf8_bytes,
            self.elapsed_monotonic_ms,
            self.retry_dispatches,
        ]
        .iter()
        .any(|ceiling| *ceiling <= 0)
        {
            return Err(Error::InvalidConfiguration);
        }
        Ok(())
    }
}

/// Immutable policy candidate. Signature syntax is checked here, but dispatch
/// needs trusted owner-key verification before accepting this as approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdvisoryBudgetPolicy {
    id: Uuid,
    version: i64,
    digest: String,
    effective_from_unix_ms: i64,
    effective_until_unix_ms: i64,
    ceilings: AdvisoryBudgetCeilings,
    approved_by: Uuid,
    approval_signature: String,
}

impl AdvisoryBudgetPolicy {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Uuid,
        version: i64,
        digest: String,
        effective_from_unix_ms: i64,
        effective_until_unix_ms: i64,
        ceilings: AdvisoryBudgetCeilings,
        approved_by: Uuid,
        approval_signature: String,
    ) -> Result<Self> {
        let policy = Self {
            id,
            version,
            digest,
            effective_from_unix_ms,
            effective_until_unix_ms,
            ceilings,
            approved_by,
            approval_signature,
        };
        policy.validate()?;
        Ok(policy)
    }

    pub fn digest_for(
        id: Uuid,
        version: i64,
        from: i64,
        until: i64,
        ceilings: AdvisoryBudgetCeilings,
    ) -> String {
        let material = format!(
            "jev-budget-policy/v1\n{id}\n{version}\n{from}\n{until}\n{}\n{}\n{}\n{}\n{}\n{}\n",
            ceilings.provider_calls,
            ceilings.input_tokens,
            ceilings.output_tokens,
            ceilings.request_utf8_bytes,
            ceilings.elapsed_monotonic_ms,
            ceilings.retry_dispatches
        );
        format!("{:x}", Sha256::digest(material.as_bytes()))
    }

    pub fn validate(&self) -> Result<()> {
        self.ceilings.validate()?;
        if self.id.is_nil()
            || self.approved_by.is_nil()
            || self.version <= 0
            || self.effective_from_unix_ms < 0
            || self.effective_until_unix_ms <= self.effective_from_unix_ms
            || self.digest
                != Self::digest_for(
                    self.id,
                    self.version,
                    self.effective_from_unix_ms,
                    self.effective_until_unix_ms,
                    self.ceilings,
                )
            || self.approval_signature.len() != 128
            || !self
                .approval_signature
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::InvalidConfiguration);
        }
        Ok(())
    }

    pub fn is_effective_at(&self, unix_ms: i64) -> bool {
        self.effective_from_unix_ms <= unix_ms && unix_ms < self.effective_until_unix_ms
    }
    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn version(&self) -> i64 {
        self.version
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn effective_from_unix_ms(&self) -> i64 {
        self.effective_from_unix_ms
    }
    pub fn effective_until_unix_ms(&self) -> i64 {
        self.effective_until_unix_ms
    }
    pub fn ceilings(&self) -> AdvisoryBudgetCeilings {
        self.ceilings
    }
    pub fn approved_by(&self) -> Uuid {
        self.approved_by
    }
    pub fn approval_signature(&self) -> &str {
        &self.approval_signature
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ceilings() -> AdvisoryBudgetCeilings {
        AdvisoryBudgetCeilings {
            provider_calls: 2,
            input_tokens: 3,
            output_tokens: 4,
            request_utf8_bytes: 5,
            elapsed_monotonic_ms: 6,
            retry_dispatches: 1,
        }
    }
    #[test]
    fn requires_every_positive_ceiling_and_exact_digest_interval_and_approval_shape() {
        let id = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let c = ceilings();
        let digest = AdvisoryBudgetPolicy::digest_for(id, 1, 100, 200, c);
        let make = |c, digest: String, from, until, signature: String| {
            AdvisoryBudgetPolicy::new(id, 1, digest, from, until, c, owner, signature)
        };
        let signature = "a".repeat(128);
        let good = make(c, digest.clone(), 100, 200, signature.clone()).unwrap();
        assert!(good.is_effective_at(100));
        assert!(!good.is_effective_at(200));
        assert!(make(c, "0".repeat(64), 100, 200, signature.clone()).is_err());
        assert!(make(c, digest.clone(), 200, 100, signature.clone()).is_err());
        assert!(make(c, digest.clone(), 100, 200, String::new()).is_err());
        for n in 0..6 {
            let mut bad = c;
            match n {
                0 => bad.provider_calls = 0,
                1 => bad.input_tokens = 0,
                2 => bad.output_tokens = 0,
                3 => bad.request_utf8_bytes = 0,
                4 => bad.elapsed_monotonic_ms = 0,
                _ => bad.retry_dispatches = 0,
            }
            assert!(make(bad, digest.clone(), 100, 200, signature.clone()).is_err());
        }
    }
}
