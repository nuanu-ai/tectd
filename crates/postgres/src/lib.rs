//! PostgreSQL persistence and operator-only administration.
pub mod admin;
mod programs;
mod runtime;
mod scope_candidate_store;
mod scope_candidates;
mod setup_store;
mod setups;
mod sources;
mod store;

pub use admin::Enrollment;
pub use store::PgStore;

use tect_domain::Error;

pub(crate) fn storage_error<T>(_: T) -> Error {
    Error::StorageUnavailable
}
