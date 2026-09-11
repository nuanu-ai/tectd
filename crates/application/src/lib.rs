//! Application policy and ports. Adapters depend on this crate, never the reverse.
mod ports;
mod programs;
mod service;

pub use ports::{ProgramOutputGuard, SourceInspector, Store, TransactionMode, UnitOfWork};
pub use service::WorkspaceService;

mod sources;
