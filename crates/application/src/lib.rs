//! Application policy and ports. Adapters depend on this crate, never the reverse.
mod advisory;
pub use advisory::{Sha256ScopeDigest, VerifySelectedSave};
mod advisory_ports;
mod matrix_advice_ports;
mod matrix_advice_runtime;
mod matrix_advisory_capture;
mod matrix_advisory_dispatch;
mod matrix_disposition;
mod matrix_disposition_ports;
mod matrix_planning_effect;
mod matrix_planning_effect_ports;
mod matrix_planning_selection_ports;
mod matrix_task_ports;
mod matrix_tasks;
mod matrix_verification;
mod matrix_verification_ports;
mod model_route_recommendation;
mod model_route_recommendation_ports;
mod pipeline_recommendation;
mod pipeline_recommendation_dispatch;
mod pipeline_recommendation_dispatch_ports;
mod pipeline_recommendation_ports;
mod pipeline_recommendation_runtime;
mod planning_knowledge_ports;
mod ports;
mod programs;
mod scope_advisory_orchestration;
mod scope_advisory_ports;
mod scope_advisory_runtime;
mod service;

#[doc(hidden)]
pub use advisory_ports::AdvisoryLifecycleCapability;
pub use advisory_ports::AdvisoryStore;
pub(crate) use advisory_ports::{AdvisoryProvider, DisabledAdvisoryProvider};
pub use advisory_ports::{
    DisabledMatrixAdviceProvider, MatrixAdviceProvider, MatrixProviderBinding,
    MatrixProviderRequest, MatrixProviderResponse, RevalidatedMatrixVerification,
    StoredMatrixDispatch,
};
#[doc(hidden)]
pub use matrix_advice_ports::canonical_matrix_advice_digest;
pub use matrix_advice_ports::{
    GuardedMatrixAdviceOutcome, GuardedMatrixAdviceRecord, MatrixAdviceStore,
    StoredGuardedMatrixAdviceRecord,
};
pub use matrix_advice_runtime::{
    DenyMatrixBudget, MAX_PREPARED_MATRIX_BODY_BYTES, MatrixBudgetAuthorization,
    MatrixBudgetPolicy, MatrixBudgetRequest, MatrixProviderIdentity, MatrixStartedDispatchPermit,
    PreparedMatrixAdviceAttempt,
};
pub use matrix_disposition_ports::{
    MatrixDispositionRecord, MatrixDispositionStore, RecordMatrixDisposition,
};
pub use matrix_planning_effect::{MatrixPlanningEffectRead, VerifyMatrixPlanningEffect};
pub use matrix_planning_effect_ports::{
    MatrixPlanningEffectAttestation, MatrixPlanningEffectSnapshot, MatrixPlanningEffectStore,
    MatrixPlanningEffectVerdict,
};
pub use matrix_planning_selection_ports::{
    MatrixPlanningMappedNode, MatrixPlanningSelectionLink, MatrixPlanningSelectionStore,
};
pub use matrix_task_ports::MatrixTaskStore;
pub use matrix_tasks::{
    CurrentMatrixAdvice, EngineeringAdvisoryRead, MATRIX_INPUT_SCHEMA, MatrixTaskRevision,
    RecordMatrixTask, RequestEngineeringAdvisory, canonical_matrix_input_digest,
};
pub use matrix_verification::{MatrixEvidenceReference, VerifyMatrixTask};
pub use matrix_verification_ports::{
    DisabledMatrixEvidenceValidator, MatrixEvidenceValidator, MatrixVerificationStore,
};
pub use model_route_recommendation::PrepareModelRouteRecommendation;
pub use model_route_recommendation_ports::{
    ModelRouteCatalogueProvider, ModelRoutePreparation, ModelRouteRecommendationBasis,
    ModelRouteRecommendationStore, PreparedModelRouteRecommendation,
    UnavailableModelRouteCatalogue,
};
pub use pipeline_recommendation::{
    PreparePipelineRecommendation, pipeline_recommendation_source_digest,
};
pub use pipeline_recommendation_dispatch::{PipelineRecommendationRun, RunPipelineRecommendation};
#[doc(hidden)]
pub use pipeline_recommendation_dispatch_ports::PipelineDispatchCapability;
pub use pipeline_recommendation_dispatch_ports::{
    PipelineRecommendationDispatchStore, StoredPipelineRecommendationDispatch,
};
pub use pipeline_recommendation_ports::{
    PipelineRecommendationBasis, PipelineRecommendationContext,
    PipelineRecommendationDefinitionProvider, PipelineRecommendationStore,
    PreparedPipelineRecommendation, UnavailablePipelineRecommendationDefinitions,
};
pub use pipeline_recommendation_runtime::{
    DisabledPipelineRecommendationProvider, MAX_PREPARED_PIPELINE_BODY_BYTES,
    MAX_SEALED_PIPELINE_RESPONSE_BYTES, PipelineProviderIdentity, PipelineProviderObservation,
    PipelineRecommendationProvider, PipelineStartedDispatchPermit,
    PreparedPipelineRecommendationAttempt, SealedPipelineRecommendationResponse,
};
pub use planning_knowledge_ports::PlanningKnowledgeStore;
pub use ports::{
    ProgramGuidance, ProgramOutputGuard, SourceInspector, Store, TransactionMode, UnitOfWork,
};
pub use scope_advisory_orchestration::{
    RunScopeAdvisory, ScopeAdvisoryOutcome, StartedScopeDispatchPermit,
};
pub use scope_advisory_ports::{
    GuardedScopeAdviceRecord, ScopeAdvisoryStore, ScopeCallerLinkInput, ScopeDispositionRecord,
    ScopeManifestRecord, ScopePreparedAdvisoryDisposition, ScopePreservationReceiptInput,
    ScopeVerifierReceiptInput, StoredScopeManifestRecord,
};
pub(crate) use scope_advisory_runtime::*;
#[doc(hidden)]
pub use scope_advisory_runtime::{
    AuthoredScopeAlternative, AuthoredScopeSet, DenyScopeBudget, PreparedScopeAdviceAttempt,
    ScopeAdviceProvider, ScopeAdviceProviderContext, ScopeAdviceProviderError,
    ScopeAdviceProviderFailureReason, ScopeAdviceProviderObservation, ScopeAdviceProviderRequest,
    ScopeAuthoredManifestRequest, ScopeAuthorityObservation, ScopeAuthorityObserver,
    ScopeAuthorityOutcome, ScopeAuthorityRequest, ScopeAuthorizedInvalidObservation,
    ScopeBudgetPolicy, ScopeBudgetPolicyEvaluation, ScopeBudgetRequest, ScopeManifestSupplier,
};
pub use service::WorkspaceService;

mod sources;

mod setup_ports;
pub use setup_ports::{SetupFiles, SetupOutputGuard, SetupStore};

mod scope_candidate_pages;
mod scope_candidate_ports;
pub use scope_candidate_ports::{CandidateGuidance, CandidateOutputGuard, ScopeCandidateStore};
mod candidate_delta_ports;
pub use candidate_delta_ports::CandidateDeltaStore;
mod candidate_delta;

mod native_planning_ports;
mod scope_candidates;
pub use native_planning_ports::{
    NativePlanningGuidance, NativePlanningOutputGuard, NativePlanningStore,
};
mod native_planning;
mod pipeline_execution;
mod pipeline_execution_ports;
pub use pipeline_execution::VerifiedPipelineSourceDigest;
pub use pipeline_execution_ports::{
    PipelineDefinitionProvider, PipelineExecutionOutputGuard, PipelineExecutionStore,
};
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

#[cfg(test)]
mod advisory_architecture_tests {
    fn assert_no_forbidden_imports(name: &str, source: &str, forbidden_imports: &[&str]) {
        for forbidden in forbidden_imports {
            assert!(!source.contains(forbidden), "{name} imports {forbidden}");
        }
    }

    #[test]
    fn domain_and_application_remain_inward_only() {
        let domain_manifest = include_str!("../../domain/Cargo.toml");
        let application_manifest = include_str!("../Cargo.toml");
        let application_advisory = include_str!("advisory.rs");
        let application_controlled_dispatch = include_str!("advisory/controlled_dispatch.rs");
        let application_ports = include_str!("advisory_ports.rs");
        let application_scope_ports = include_str!("scope_advisory_ports.rs");
        let application_scope_runtime = include_str!("scope_advisory_runtime.rs");
        let application_scope_orchestration = include_str!("scope_advisory_orchestration.rs");
        let application_scope_capture = include_str!("scope_advisory_orchestration/capture.rs");
        let application_scope_decisions = include_str!("scope_advisory_orchestration/decisions.rs");
        let application_scope_helpers = include_str!("scope_advisory_orchestration/helpers.rs");
        let architecture_check = include_str!("../../../scripts/check-architecture.py");
        for (name, source) in [
            ("domain manifest", domain_manifest),
            ("application manifest", application_manifest),
        ] {
            assert_no_forbidden_imports(
                name,
                source,
                &["sqlx", "reqwest", "hyper", "tect-postgres", "tect-host"],
            );
        }
        for (name, source) in [
            ("application advisory", application_advisory),
            (
                "application controlled dispatch",
                application_controlled_dispatch,
            ),
            ("application advisory ports", application_ports),
            ("application scope advisory ports", application_scope_ports),
            (
                "application scope advisory runtime",
                application_scope_runtime,
            ),
            (
                "application scope advisory orchestration",
                application_scope_orchestration,
            ),
            (
                "application scope advisory capture",
                application_scope_capture,
            ),
            (
                "application scope advisory decisions",
                application_scope_decisions,
            ),
            (
                "application scope advisory helpers",
                application_scope_helpers,
            ),
        ] {
            assert_no_forbidden_imports(
                name,
                source,
                &[
                    "sqlx::",
                    concat!("std", "::env"),
                    "reqwest::",
                    "hyper::",
                    "tect_postgres",
                    "tect_host",
                    "JevDto",
                ],
            );
        }
        assert!(architecture_check.contains("domain_advisory_sources"));
        assert!(architecture_check.contains("rglob(\"*.rs\")"));
        assert!(architecture_check.contains("if not is_test_fixture(source)"));
    }

    #[test]
    fn host_and_postgres_are_outward_adapters() {
        let postgres_manifest = include_str!("../../postgres/Cargo.toml");
        let host_manifest = include_str!("../../host/Cargo.toml");
        for manifest in [postgres_manifest, host_manifest] {
            assert!(manifest.contains("tect-domain.workspace = true"));
            assert!(manifest.contains("tect-application.workspace = true"));
        }
        assert!(postgres_manifest.contains("sqlx.workspace = true"));
        assert!(!host_manifest.contains("tect-postgres.workspace = true"));
    }
}

mod setup_access;
mod setup_apply;
mod setups;
