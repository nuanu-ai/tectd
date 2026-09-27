use super::*;
use std::{path::Path, sync::Arc};

pub(super) fn identity(profile: &str) -> PipelineProviderIdentity {
    PipelineProviderIdentity {
        provider: profile.into(),
        model: REAL_MODEL.into(),
        destination: ENDPOINT.into(),
        wire_version: WIRE_VERSION.into(),
    }
}

pub(super) async fn pipeline_service(
    runtime_url: &str,
    keys: BudgetOwnerKeys,
    task: Uuid,
    matrix: &EngineeringMatrixInput,
    provider: Arc<dyn tect_application::PipelineRecommendationProvider>,
) -> Arc<WorkspaceService> {
    Arc::new(
        WorkspaceService::new(
            Arc::new(
                PgStore::connect(runtime_url, 4)
                    .await
                    .unwrap()
                    .with_budget_owner_keys(keys),
            ),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(Evidence))
        .with_pipeline_recommendation_definitions(Arc::new(
            tect_host::StaticPipelineRecommendationDefinitions,
        ))
        .with_pipeline_compatibility_policy(Arc::new(FixedPipelineCompatibilityPolicy(policy(
            task, matrix,
        ))))
        .with_pipeline_recommendation_provider(provider),
    )
}

pub(super) async fn start_server(
    path: &Path,
    service: Arc<WorkspaceService>,
) -> tokio::task::JoinHandle<tect_domain::Result<()>> {
    let listener = UnixListener::bind(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    tokio::spawn(tect_host::serve(listener, service))
}
