use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub const SHA256_HEX_BYTES: usize = 64;

/// Inward digest port. Implementations live in the application layer; the
/// domain owns canonical bytes and a distinct domain-separation label.
pub trait ScopeDigest {
    fn sha256(&self, domain: &'static str, canonical_bytes: &[u8]) -> String;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceApplicability {
    Applicable,
    KnownEmpty,
    Unknown,
    Gap,
    Conflict,
    Invalid,
}

impl SourceApplicability {
    pub const fn permits_advisory(self) -> bool {
        matches!(self, Self::Applicable | Self::KnownEmpty)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenSourceInput {
    pub id: String,
    pub version: String,
    pub digest: String,
    pub provenance: String,
    pub applicability: SourceApplicability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceClause {
    pub id: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceObligation {
    pub id: String,
    pub source_input_id: String,
    pub statement_digest: String,
    #[serde(default)]
    pub conditions: Vec<SourceClause>,
    #[serde(default)]
    pub exceptions: Vec<SourceClause>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenScopeSource {
    pub candidate_set_id: Uuid,
    pub candidate_set_revision: i64,
    pub snapshot_id: Uuid,
    pub input_cursor: i64,
    pub program_id: Uuid,
    pub program_revision: i64,
    pub program_latest_input: i64,
    pub planning_latest_input: i64,
    pub selected_sources_digest: String,
    pub method_revision: String,
    pub method_digest: String,
    pub registry_revision: String,
    pub registry_digest: String,
    pub inputs: Vec<FrozenSourceInput>,
    pub digest: String,
}

impl FrozenScopeSource {
    pub fn validate(&self, digest: &impl ScopeDigest) -> Result<()> {
        if self.candidate_set_id.is_nil()
            || self.snapshot_id.is_nil()
            || self.program_id.is_nil()
            || self.candidate_set_revision < 1
            || self.program_revision < 1
            || self.input_cursor < 0
            || self.program_latest_input < 0
            || self.planning_latest_input < 0
            || !valid_digest(&self.selected_sources_digest)
            || !valid_text(&self.method_revision)
            || !valid_digest(&self.method_digest)
            || !valid_text(&self.registry_revision)
            || !valid_digest(&self.registry_digest)
            || self.inputs.is_empty()
        {
            return Err(Error::InvalidArguments);
        }
        let mut ids = BTreeSet::new();
        let ordered_ids = self
            .inputs
            .iter()
            .map(|value| &value.id)
            .collect::<Vec<_>>();
        let mut sorted_ids = ordered_ids.clone();
        sorted_ids.sort();
        if ordered_ids != sorted_ids {
            return Err(Error::InvalidArguments);
        }
        for input in &self.inputs {
            if !valid_id(&input.id)
                || !valid_text(&input.version)
                || !valid_digest(&input.digest)
                || !valid_text(&input.provenance)
                || !ids.insert(&input.id)
            {
                return Err(Error::InvalidArguments);
            }
            if !input.applicability.permits_advisory() {
                return Err(Error::InvalidSource);
            }
        }
        if self.digest != self.canonical_digest(digest)? {
            return Err(Error::InputConflict);
        }
        Ok(())
    }

    pub fn canonical_digest(&self, digest: &impl ScopeDigest) -> Result<String> {
        canonical_digest(
            digest,
            "tect.frozen-scope-source/1",
            &(
                self.candidate_set_id,
                self.candidate_set_revision,
                self.snapshot_id,
                self.input_cursor,
                self.program_id,
                self.program_revision,
                self.program_latest_input,
                self.planning_latest_input,
                &self.selected_sources_digest,
                &self.method_revision,
                &self.method_digest,
                &self.registry_revision,
                &self.registry_digest,
                &self.inputs,
            ),
        )
    }
}

pub(crate) fn validate_obligations(
    source: &FrozenScopeSource,
    obligations: &[SourceObligation],
) -> Result<()> {
    if obligations.is_empty() {
        return Err(Error::InvalidSource);
    }
    let ordered_ids = obligations
        .iter()
        .map(|value| &value.id)
        .collect::<Vec<_>>();
    let mut sorted_ids = ordered_ids.clone();
    sorted_ids.sort();
    if ordered_ids != sorted_ids {
        return Err(Error::InvalidArguments);
    }
    let source_inputs = source
        .inputs
        .iter()
        .map(|value| (&value.id, value.applicability))
        .collect::<BTreeMap<_, _>>();
    let mut obligation_ids = BTreeSet::new();
    for obligation in obligations {
        if !valid_id(&obligation.id)
            || source_inputs.get(&obligation.source_input_id)
                != Some(&SourceApplicability::Applicable)
            || !valid_digest(&obligation.statement_digest)
            || !obligation_ids.insert(&obligation.id)
        {
            return Err(Error::InvalidArguments);
        }
        validate_clauses(&obligation.conditions)?;
        validate_clauses(&obligation.exceptions)?;
    }
    Ok(())
}

fn validate_clauses(clauses: &[SourceClause]) -> Result<()> {
    let mut ids = BTreeSet::new();
    if clauses
        .iter()
        .any(|value| !valid_id(&value.id) || !valid_digest(&value.digest) || !ids.insert(&value.id))
    {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

pub(crate) fn canonical_digest(
    digest: &impl ScopeDigest,
    domain: &'static str,
    value: &impl Serialize,
) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::InternalInvariant)?;
    let value = digest.sha256(domain, &bytes);
    if !valid_digest(&value) {
        return Err(Error::InternalInvariant);
    }
    Ok(value)
}

pub(crate) fn valid_digest(value: &str) -> bool {
    value.len() == SHA256_HEX_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(crate) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256
}
