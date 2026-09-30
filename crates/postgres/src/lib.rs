//! PostgreSQL persistence and operator-only administration.
pub mod admin;
mod advisory;
#[cfg(test)]
mod advisory_budget_consumption_migration_tests;
mod advisory_budget_policy_approval;
#[cfg(test)]
mod advisory_budget_policy_global_migration_tests;
#[cfg(test)]
mod advisory_budget_policy_migration_tests;
mod advisory_budget_policy_store;
#[cfg(test)]
mod advisory_budget_reservation_migration_tests;
#[cfg(test)]
mod advisory_migration_tests;
#[cfg(test)]
mod anti_bloat_migration_tests;
mod anti_bloat_store;
mod anti_bloat_verification_store;
#[cfg(test)]
mod authenticated_host_wire_pg_tests;
mod budget_owner_keys;
mod budget_policy_usage;
#[cfg(test)]
mod context_matrix_verification_migration_tests;
mod context_matrix_verification_store;
mod durable_knowledge;
mod durable_knowledge_admin;
mod durable_knowledge_store;
#[cfg(test)]
mod engineering_profile_opportunity_migration_tests;
mod knowledge_lifecycle;
mod knowledge_lifecycle_store;
pub(crate) mod knowledge_maintenance;
mod knowledge_maintenance_store;
mod knowledge_recovery;
mod knowledge_search;
mod knowledge_search_admin;
mod knowledge_search_store;
#[cfg(test)]
mod matrix_advice_pg_tests;
mod matrix_advice_store;
#[cfg(test)]
mod matrix_choice_set_migration_tests;
#[cfg(test)]
mod matrix_disposition_migration_tests;
mod matrix_disposition_store;
mod matrix_evidence_artifact;
#[cfg(test)]
mod matrix_lock_contention_pg_tests;
#[cfg(test)]
mod matrix_lock_contention_tests;
#[cfg(test)]
mod matrix_no_choice_migration_tests;
#[cfg(test)]
mod matrix_planning_effect_migration_tests;
mod matrix_planning_effect_store;
#[cfg(test)]
mod matrix_planning_selection_migration_tests;
mod matrix_planning_selection_store;
#[cfg(test)]
mod matrix_requirements_context_migration_tests;
mod matrix_requirements_context_store;
#[cfg(test)]
mod matrix_task_migration_tests;
#[cfg(test)]
mod matrix_task_pg_tests;
mod matrix_task_store;
#[cfg(test)]
mod matrix_v1_cutover_migration_tests;
#[cfg(test)]
mod matrix_verification_migration_tests;
#[cfg(test)]
mod matrix_verification_pg_tests;
mod matrix_verification_store;
mod model_route_attempt_store;
#[cfg(test)]
mod model_route_live_tests;
#[cfg(test)]
mod model_route_migration_tests;
mod model_route_selection_read;
mod model_route_store;
mod native_planning;
mod native_planning_store;
#[cfg(test)]
mod pipeline_advice_manifest_migration_tests;
#[cfg(test)]
mod pipeline_advice_migration_tests;
#[cfg(test)]
mod pipeline_advice_ready_draft_migration_tests;
#[cfg(test)]
mod pipeline_dispatch_migration_tests;
mod pipeline_disposition_store;
mod pipeline_execution;
mod pipeline_execution_store;
mod pipeline_open_effect_store;
#[cfg(test)]
mod pipeline_phase_effect_migration_tests;
mod pipeline_phase_effect_store;
mod pipeline_recommendation_store;
mod planning_knowledge;
mod planning_knowledge_store;
mod programs;
#[cfg(test)]
mod provider_observation_migration_tests;
mod runtime;
mod scope_advisory;
#[cfg(test)]
mod scope_advisory_migration_tests;
mod scope_candidate_store;
mod scope_candidates;
#[cfg(test)]
mod session_advisory_preference_migration_tests;
mod setup_store;
mod setups;
mod sources;
mod store;
#[cfg(test)]
mod technical_decision_comparison_pg_tests;
mod technical_decision_evidence;

pub use admin::Enrollment;
pub use advisory_budget_policy_approval::verify_budget_policy_approval;
pub use budget_owner_keys::BudgetOwnerKeys;
pub use durable_knowledge_admin::enable_durable_knowledge;
pub use knowledge_recovery::{
    KnowledgeAdminPool, KnowledgeDatabaseIdentity, KnowledgeRecoveryReport,
    KnowledgeSuppressionCheckpoint, KnowledgeSuppressionEntry, KnowledgeSuppressionManifest,
    apply_knowledge_suppression_manifest, current_knowledge_database_identity,
    knowledge_suppression_manifest_bytes, parse_knowledge_suppression_manifest,
    prepare_knowledge_suppression_manifest, record_knowledge_suppression_export,
};
pub use knowledge_search_admin::enable_knowledge_vector_search;
pub use matrix_evidence_artifact::{ApprovedMatrixEvidenceArtifact, PgMatrixEvidenceValidator};
pub use scope_advisory::{PgScopeAuthoredManifestSupplier, PgScopeAuthorityObserver};
pub use store::PgStore;
pub use technical_decision_evidence::{
    ApprovedTechnicalDecisionEvidence, PgTechnicalDecisionEvidenceResolver,
    TECHNICAL_EVIDENCE_FORMAT, TECHNICAL_EVIDENCE_SCHEMA, TechnicalDecisionEvidenceArtifact,
    TechnicalEvidenceCandidateMapping, TechnicalEvidenceLocator, TechnicalEvidenceObservation,
    TechnicalEvidenceRequirements, parse_technical_decision_approvals,
};

use tect_domain::Error;

pub(crate) fn storage_error<T>(_: T) -> Error {
    Error::StorageUnavailable
}

// Only matrix task head locks use this mapping. NOWAIT contention aborts the
// PostgreSQL transaction and is a stale attempt, not a storage outage.
pub(crate) fn matrix_lock_error(error: sqlx::Error) -> Error {
    if error
        .as_database_error()
        .and_then(|error| error.code())
        .as_deref()
        == Some("55P03")
    {
        Error::StaleRevision
    } else {
        storage_error(error)
    }
}
