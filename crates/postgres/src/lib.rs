//! PostgreSQL persistence and operator-only administration.
pub mod admin;
mod advisory;
#[cfg(test)]
mod advisory_migration_tests;
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
mod matrix_no_choice_migration_tests;
#[cfg(test)]
mod matrix_task_migration_tests;
#[cfg(test)]
mod matrix_task_pg_tests;
mod matrix_task_store;
#[cfg(test)]
mod matrix_verification_migration_tests;
mod matrix_verification_store;
mod native_planning;
mod native_planning_store;
mod pipeline_execution;
mod pipeline_execution_store;
mod planning_knowledge;
mod planning_knowledge_store;
mod programs;
mod runtime;
mod scope_advisory;
#[cfg(test)]
mod scope_advisory_migration_tests;
mod scope_candidate_store;
mod scope_candidates;
mod setup_store;
mod setups;
mod sources;
mod store;

pub use admin::Enrollment;
pub use durable_knowledge_admin::enable_durable_knowledge;
pub use knowledge_recovery::{
    KnowledgeAdminPool, KnowledgeDatabaseIdentity, KnowledgeRecoveryReport,
    KnowledgeSuppressionCheckpoint, KnowledgeSuppressionEntry, KnowledgeSuppressionManifest,
    apply_knowledge_suppression_manifest, current_knowledge_database_identity,
    knowledge_suppression_manifest_bytes, parse_knowledge_suppression_manifest,
    prepare_knowledge_suppression_manifest, record_knowledge_suppression_export,
};
pub use knowledge_search_admin::enable_knowledge_vector_search;
pub use scope_advisory::{PgScopeAuthoredManifestSupplier, PgScopeAuthorityObserver};
pub use store::PgStore;

use tect_domain::Error;

pub(crate) fn storage_error<T>(_: T) -> Error {
    Error::StorageUnavailable
}
