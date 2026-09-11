//! Application policy and ports. Adapters depend on this crate, never the reverse.
mod ports;
mod service;

pub use ports::{SourceInspector, Store, TransactionMode, UnitOfWork};
pub use service::WorkspaceService;

mod sources;
