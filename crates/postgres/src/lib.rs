//! PostgreSQL persistence and operator-only administration.
pub mod admin;
mod sources;
mod store;

pub use admin::Enrollment;
pub use store::PgStore;

use tect_domain::Error;

pub(crate) fn storage_error<T>(_: T) -> Error {
    Error::StorageUnavailable
}
