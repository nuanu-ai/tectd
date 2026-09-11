//! Application policy and ports. Adapters depend on this crate, never the reverse.
mod ports;
mod service;

pub use ports::{Store, TransactionMode, UnitOfWork};
pub use service::WorkspaceService;
