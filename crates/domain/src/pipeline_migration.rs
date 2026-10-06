use crate::{Error, PipelineDefinitionSnapshot, PipelineKind, RefusalCode, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const CURRENT_LIGHTWEIGHT_VERSION: &str = "0.7.1-native.k1k5";
pub const CURRENT_LIGHTWEIGHT_DEFINITION_DIGEST: &str =
    "93df97f4cb4458a18411b76005b29025a56234dc47650e4147ac5fdab3d30d89";

/// Infrastructure supplies SHA256 without adding a hashing dependency to the domain.
pub trait PipelineDefinitionDigestPort {
    fn sha256(&self, canonical_json: &[u8]) -> [u8; 32];
}

/// Hash the typed snapshot exactly as the immutable provider does. JSON field
/// order comes from serialization of the snapshot, not from the source file.
pub fn pipeline_definition_digest(
    definition: &PipelineDefinitionSnapshot,
    digest: &dyn PipelineDefinitionDigestPort,
) -> Result<String> {
    let mut material = definition.clone();
    material.digest.clear();
    let bytes = serde_json::to_vec(&material).map_err(|_| Error::InvalidConfiguration)?;
    Ok(digest
        .sha256(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub fn is_current_lightweight_retirement_successor(
    definition: &PipelineDefinitionSnapshot,
    digest: &dyn PipelineDefinitionDigestPort,
) -> bool {
    definition.kind == PipelineKind::LightweightTddDevelopment
        && definition.version == CURRENT_LIGHTWEIGHT_VERSION
        && definition.phases.len() == 5
        && definition.digest == CURRENT_LIGHTWEIGHT_DEFINITION_DIGEST
        && pipeline_definition_digest(definition, digest)
            .is_ok_and(|digest| digest == CURRENT_LIGHTWEIGHT_DEFINITION_DIGEST)
}

/// Retag only this retirement rule for a successor selector. Retain outer
/// compatibility wrappers and every other diagnostic field unchanged.
pub fn migration_successor_retirement_error(mut error: Error) -> Error {
    fn retag(refusal: &mut crate::Refusal) {
        if refusal.rule.as_deref() == Some("WP6-LIGHTWEIGHT-RETIRED-01") {
            refusal.path = Some("arguments.params.successor_definition_version".into());
            refusal.next_action = Some("get_current_context_and_use_exact_migration_action".into());
        }
    }
    fn visit(error: &mut Error) {
        match error {
            Error::Refused(refusal) => retag(refusal),
            Error::PipelineRefused { source, refusal } => {
                visit(source);
                retag(refusal);
            }
            _ => {}
        }
    }
    visit(&mut error);
    error
}

/// Retirement is scoped to the stored Lightweight snapshot, never its version alone.
pub fn is_retired_lightweight(definition: &PipelineDefinitionSnapshot) -> bool {
    definition.kind == PipelineKind::LightweightTddDevelopment
        && (definition.phases.len() == 15
            || matches!(
                definition.version.as_str(),
                "0.6.0-native.engineering.2" | "0.4.0-native.skills.1" | "0.1.0-native.1"
            ))
}

pub fn lightweight_retirement_error(
    path: &'static str,
    actual: impl Into<String>,
    next: &'static str,
) -> Error {
    Error::refused_at(
        RefusalCode::LegacyMigrationRequired,
        "WP6-LIGHTWEIGHT-RETIRED-01",
        path,
        "current Lightweight K1-K5 0.7.1-native.k1k5",
        actual,
        next,
        "current_lightweight_k1k5",
    )
}

pub fn ensure_pipeline_definition_selectable(
    definition: &PipelineDefinitionSnapshot,
) -> Result<()> {
    if is_retired_lightweight(definition) {
        return Err(lightweight_retirement_error(
            "arguments.params.definition_version",
            definition.version.clone(),
            "begin_current_lightweight_k1k5",
        ));
    }
    Ok(())
}

pub fn ensure_pipeline_run_mutable(
    definition: &PipelineDefinitionSnapshot,
    status: &str,
) -> Result<()> {
    if status == "superseded" || is_retired_lightweight(definition) {
        return Err(lightweight_retirement_error(
            "arguments.params.run_id",
            status.to_owned(),
            "get_current_context_and_use_exact_migration_action",
        ));
    }
    Ok(())
}

/// Caller-facing migration command. The predecessor definition metadata is
/// resolved from the immutable stored run; callers may only select the
/// successor snapshot and provide explicit obligation/evidence mappings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineRunMigrationCommand {
    pub request_id: Uuid,
    pub predecessor_run_id: Uuid,
    pub expected_revision: i64,
    pub idempotency_key: String,
    pub successor_definition_version: String,
    pub mappings: Vec<PipelineObligationMapping>,
}

impl PipelineRunMigrationCommand {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.predecessor_run_id.is_nil()
            || self.expected_revision < 1
            || self.idempotency_key.is_empty()
            || self.idempotency_key.len() > 128
            || self.successor_definition_version.trim().is_empty()
        {
            return Err(Error::InvalidArguments);
        }
        if self.mappings.is_empty() {
            Ok(())
        } else {
            validate_mappings(&self.mappings)
        }
    }
}

/// Explicit compatibility metadata for moving a legacy run to a new
/// definition. A run's persisted definition is never rewritten by this
/// contract; the migration creates a distinct successor run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineRunMigrationRequest {
    pub request_id: Uuid,
    pub predecessor_run_id: Uuid,
    pub predecessor_definition_version: String,
    pub predecessor_definition_digest: String,
    pub successor_definition_version: String,
    pub successor_definition_digest: String,
    pub mappings: Vec<PipelineObligationMapping>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineObligationMapping {
    pub legacy_obligation_id: String,
    pub successor_obligation_id: String,
    pub evidence_refs: Vec<PipelineMigrationEvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineMigrationEvidenceRef {
    pub reference: String,
    pub digest: String,
}

impl PipelineRunMigrationRequest {
    pub fn validate(&self) -> Result<()> {
        self.validate_metadata()?;
        validate_mappings(&self.mappings)
    }

    /// Empty mappings are authorized only after actual immutable snapshots are resolved.
    pub fn validate_retirement_restart(
        &self,
        predecessor: &PipelineDefinitionSnapshot,
        successor: &PipelineDefinitionSnapshot,
        digest: &dyn PipelineDefinitionDigestPort,
    ) -> Result<()> {
        if !is_retired_lightweight(predecessor) {
            return self.validate();
        }
        if !is_current_lightweight_retirement_successor(successor, digest) {
            return Err(retirement_restart_refusal(
                "WP6-MIGRATION-CONTRACT-01",
                "arguments.params.successor_definition_version",
                "noncanonical retirement successor",
            ));
        }
        if self.validate_metadata().is_err()
            || self.predecessor_definition_version != predecessor.version
            || self.predecessor_definition_digest != predecessor.digest
            || self.successor_definition_version != successor.version
            || self.successor_definition_digest != successor.digest
        {
            return Err(retirement_restart_refusal(
                "WP6-MIGRATION-CONTRACT-01",
                "arguments.params",
                "invalid retirement contract",
            ));
        }
        if !self.mappings.is_empty() {
            return Err(retirement_restart_refusal(
                "WP6-MIGRATION-MAPPING-01",
                "arguments.params.mappings",
                self.mappings.len().to_string(),
            ));
        }
        Ok(())
    }

    fn validate_metadata(&self) -> Result<()> {
        let valid = !self.request_id.is_nil()
            && !self.predecessor_run_id.is_nil()
            && nonempty(&self.predecessor_definition_version)
            && nonempty(&self.predecessor_definition_digest)
            && nonempty(&self.successor_definition_version)
            && nonempty(&self.successor_definition_digest)
            && self.predecessor_definition_version != self.successor_definition_version
            && self.predecessor_definition_digest != self.successor_definition_digest;
        if valid {
            Ok(())
        } else {
            Err(Error::refused_at(
                RefusalCode::LegacyMigrationRequired,
                "WP6-MIGRATION-CONTRACT-01",
                "arguments.params",
                "distinct predecessor/successor identities and complete obligation/evidence mappings",
                "invalid or incomplete migration contract",
                "provide_explicit_successor_mapping",
                "predecessor_successor_obligation_evidence_metadata",
            ))
        }
    }
}

fn retirement_restart_refusal(
    rule: &'static str,
    path: &'static str,
    actual: impl Into<String>,
) -> Error {
    Error::refused_at(
        RefusalCode::LegacyMigrationRequired,
        rule,
        path,
        "canonical current Lightweight K1-K5 0.7.1-native.k1k5 with empty mappings",
        actual,
        "get_current_context_and_use_exact_migration_action",
        "canonical_current_lightweight_k1k5_empty_mappings",
    )
}

fn validate_mappings(mappings: &[PipelineObligationMapping]) -> Result<()> {
    let duplicate = mappings
        .iter()
        .map(|mapping| mapping.legacy_obligation_id.as_str())
        .collect::<Vec<_>>();
    let unique = duplicate.iter().copied().collect::<BTreeSet<_>>().len();
    let valid = !mappings.is_empty()
        && unique == mappings.len()
        && mappings.iter().all(|mapping| {
            nonempty(&mapping.legacy_obligation_id)
                && nonempty(&mapping.successor_obligation_id)
                && !mapping.evidence_refs.is_empty()
                && mapping
                    .evidence_refs
                    .iter()
                    .all(|evidence| nonempty(&evidence.reference) && nonempty(&evidence.digest))
        });
    valid.then_some(()).ok_or_else(|| {
        Error::refused_at(
            RefusalCode::LegacyMigrationRequired,
            "WP6-MIGRATION-MAPPING-01",
            "arguments.params.mappings",
            "unique predecessor and successor obligations with non-empty evidence refs",
            "duplicate or incomplete mapping",
            "provide_explicit_successor_mapping",
            "predecessor_successor_obligation_evidence_metadata",
        )
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineRunMigrationOutcome {
    pub migration_id: Uuid,
    pub predecessor_run_id: Uuid,
    pub successor_run_id: Uuid,
    pub predecessor_revision: i64,
    pub successor_definition_version: String,
    pub successor_definition_digest: String,
    pub status: String,
}

const fn nonempty(value: &str) -> bool {
    !value.is_empty()
}

#[cfg(test)]
mod tests;
