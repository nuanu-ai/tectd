//! PostgreSQL persistence and operator-only administration.
pub mod admin;
mod durable_knowledge;
mod durable_knowledge_admin;
mod durable_knowledge_store;
mod knowledge_lifecycle;
mod knowledge_lifecycle_store;
mod knowledge_recovery;
mod native_planning;
mod native_planning_store;
mod pipeline_execution;
mod pipeline_execution_store;
mod programs;
mod runtime;
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
pub use store::PgStore;

use tect_domain::Error;

pub(crate) fn storage_error<T>(_: T) -> Error {
    Error::StorageUnavailable
}
