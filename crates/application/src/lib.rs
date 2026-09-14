//! Application policy and ports. Adapters depend on this crate, never the reverse.
mod planning_knowledge_ports;
mod ports;
mod programs;
mod service;

pub use planning_knowledge_ports::PlanningKnowledgeStore;
pub use ports::{
    ProgramGuidance, ProgramOutputGuard, SourceInspector, Store, TransactionMode, UnitOfWork,
};
pub use service::WorkspaceService;

mod sources;

mod setup_ports;
pub use setup_ports::{SetupFiles, SetupOutputGuard, SetupStore};

mod scope_candidate_pages;
mod scope_candidate_ports;
pub use scope_candidate_ports::{CandidateGuidance, CandidateOutputGuard, ScopeCandidateStore};

mod native_planning_ports;
mod scope_candidates;
pub use native_planning_ports::{
    NativePlanningGuidance, NativePlanningOutputGuard, NativePlanningStore,
};
mod native_planning;
mod pipeline_execution;
mod pipeline_execution_ports;
pub use pipeline_execution_ports::{PipelineDefinitionProvider, PipelineExecutionStore};
mod durable_knowledge;
mod durable_knowledge_ports;
pub use durable_knowledge_ports::DurableKnowledgeStore;
mod knowledge_lifecycle;
mod knowledge_lifecycle_ports;
pub use knowledge_lifecycle_ports::{
    KnowledgeLifecycleDefinitionProvider, KnowledgeLifecycleStore, KnowledgeOutputGuard,
};
mod knowledge_maintenance;
mod knowledge_maintenance_ports;
pub use knowledge_maintenance_ports::{KnowledgeMaintenanceOutputGuard, KnowledgeMaintenanceStore};
mod knowledge_search;
mod knowledge_search_ports;
pub use knowledge_search_ports::{
    DisabledKnowledgeEmbeddingProvider, KnowledgeEmbeddingProvider, KnowledgeSearchOutputGuard,
    KnowledgeSearchStore,
};

mod setup_access;
mod setup_apply;
mod setups;
