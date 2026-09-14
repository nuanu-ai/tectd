//! PostgreSQL persistence and operator-only administration.
pub mod admin;
mod durable_knowledge;
mod durable_knowledge_admin;
mod durable_knowledge_store;
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
pub use store::PgStore;

use tect_domain::Error;

pub(crate) fn storage_error<T>(_: T) -> Error {
    Error::StorageUnavailable
}
