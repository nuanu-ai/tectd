//! Application policy and ports. Adapters depend on this crate, never the reverse.
mod ports;
mod programs;
mod service;

pub use ports::{ProgramOutputGuard, SourceInspector, Store, TransactionMode, UnitOfWork};
pub use service::WorkspaceService;

mod sources;

mod setup_ports;
pub use setup_ports::{SetupFiles, SetupOutputGuard, SetupStore};

mod scope_candidate_pages;
mod scope_candidate_ports;
pub use scope_candidate_ports::{CandidateGuidance, CandidateOutputGuard, ScopeCandidateStore};

mod native_planning_ports;
mod scope_candidates;
pub use native_planning_ports::{NativePlanningGuidance, NativePlanningStore};
mod native_planning;

mod setup_access;
mod setup_apply;
mod setups;
