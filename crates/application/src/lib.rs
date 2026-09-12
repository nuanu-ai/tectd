//! Application policy and ports. Adapters depend on this crate, never the reverse.
mod ports;
mod programs;
mod service;

pub use ports::{ProgramOutputGuard, SourceInspector, Store, TransactionMode, UnitOfWork};
pub use service::WorkspaceService;

mod sources;

mod setup_ports;
pub use setup_ports::{SetupFiles, SetupOutputGuard, SetupStore};

mod setup_access;
mod setup_apply;
mod setups;
