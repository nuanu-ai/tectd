use crate::{Error, RefusalCode, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

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
        validate_mappings(&self.mappings)
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
        let valid = !self.request_id.is_nil()
            && !self.predecessor_run_id.is_nil()
            && nonempty(&self.predecessor_definition_version)
            && nonempty(&self.predecessor_definition_digest)
            && nonempty(&self.successor_definition_version)
            && nonempty(&self.successor_definition_digest)
            && self.predecessor_definition_version != self.successor_definition_version
            && self.predecessor_definition_digest != self.successor_definition_digest
            && validate_mappings(&self.mappings).is_ok();
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
mod tests {
    use super::*;

    fn request() -> PipelineRunMigrationRequest {
        PipelineRunMigrationRequest {
            request_id: Uuid::new_v4(),
            predecessor_run_id: Uuid::new_v4(),
            predecessor_definition_version: "0.6".into(),
            predecessor_definition_digest: "legacy-digest".into(),
            successor_definition_version: "0.7".into(),
            successor_definition_digest: "successor-digest".into(),
            mappings: vec![PipelineObligationMapping {
                legacy_obligation_id: "phase-01".into(),
                successor_obligation_id: "checkpoint-01".into(),
                evidence_refs: vec![PipelineMigrationEvidenceRef {
                    reference: "artifact://evidence/1".into(),
                    digest: "evidence-digest".into(),
                }],
            }],
        }
    }

    #[test]
    fn explicit_mapping_is_accepted() {
        let migration = request();
        let predecessor = (
            migration.predecessor_run_id,
            migration.predecessor_definition_version.clone(),
            migration.predecessor_definition_digest.clone(),
        );
        assert!(migration.validate().is_ok());
        // Validation is additive metadata only: it cannot rewrite the legacy
        // run identity or its pinned v0.6 definition.
        assert_eq!(
            predecessor,
            (
                migration.predecessor_run_id,
                migration.predecessor_definition_version,
                migration.predecessor_definition_digest
            )
        );
    }

    #[test]
    fn missing_or_ambiguous_mapping_is_refused() {
        let mut missing = request();
        missing.mappings.clear();
        let error = missing.validate().unwrap_err();
        assert_eq!(error.code(), "LEGACY_MIGRATION_REQUIRED");

        let mut ambiguous = request();
        ambiguous.mappings.push(ambiguous.mappings[0].clone());
        let error = ambiguous.validate().unwrap_err();
        assert_eq!(error.code(), "LEGACY_MIGRATION_REQUIRED");
    }

    #[test]
    fn predecessor_definition_cannot_be_reinterpreted_as_successor() {
        let mut request = request();
        request.successor_definition_version = request.predecessor_definition_version.clone();
        assert_eq!(
            request.validate().unwrap_err().code(),
            "LEGACY_MIGRATION_REQUIRED"
        );
    }
}
