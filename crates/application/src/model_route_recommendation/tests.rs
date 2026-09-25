use super::*;
use crate::{
    ModelRouteCatalogueProvider, ModelRouteRecommendationBasis, ModelRouteRecommendationStore,
    PreparedModelRouteRecommendation,
};
use async_trait::async_trait;
use tect_domain::{
    MODEL_ROUTE_CATALOGUE_SCHEMA, MatrixPlanningSelection, ModelRoute, ModelRouteCatalogue,
    ModelRouteWorkContext, ObservedModelRoute,
};

struct Catalogue(Option<ModelRouteCatalogue>);

impl ModelRouteCatalogueProvider for Catalogue {
    fn catalogue(&self) -> Result<Option<ModelRouteCatalogue>> {
        Ok(self.0.clone())
    }
}

#[derive(Default)]
struct Store {
    basis: Option<ModelRouteRecommendationBasis>,
    saved: Option<PreparedModelRouteRecommendation>,
    captures: usize,
}

#[async_trait]
impl ModelRouteRecommendationStore for Store {
    async fn by_request(
        &mut self,
        _workspace_id: Uuid,
        _request_key: &str,
    ) -> Result<Option<PreparedModelRouteRecommendation>> {
        Ok(self.saved.clone())
    }

    async fn load_basis(
        &mut self,
        _workspace_id: Uuid,
        _disposition_id: Uuid,
    ) -> Result<Option<ModelRouteRecommendationBasis>> {
        Ok(self.basis.clone())
    }

    async fn capture(
        &mut self,
        prepared: &PreparedModelRouteRecommendation,
    ) -> Result<PreparedModelRouteRecommendation> {
        self.captures += 1;
        self.saved = Some(prepared.clone());
        Ok(prepared.clone())
    }
}

fn fixture() -> (PrepareModelRouteRecommendation, Store, Catalogue) {
    let selection = MatrixPlanningSelection {
        task_id: Uuid::new_v4(),
        task_revision: 3,
        disposition_id: Uuid::new_v4(),
        selected_choice_id: "choice-a".into(),
        expected_input_digest: "a".repeat(64),
        expected_choice_set_digest: "b".repeat(64),
        expected_verification_digest: "c".repeat(64),
        mapped_draft_node_indices: vec![0],
    };
    let request = PrepareModelRouteRecommendation {
        workspace_id: Uuid::new_v4(),
        disposition_id: selection.disposition_id,
        expected_task_id: selection.task_id,
        expected_task_revision: selection.task_revision,
        request_key: "request-1".into(),
        requested_route_id: Some("route-disabled".into()),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
    };
    let basis = ModelRouteRecommendationBasis {
        work: ModelRouteWorkContext {
            approved_matrix_selection: selection,
            role: "agent".into(),
            tool: "code".into(),
            data_class: "internal".into(),
            host_capabilities: vec!["model-api".into()],
            remaining_budget_units: 20,
            available_latency_ms: 100,
        },
        advisory_mode: WorkspaceAdvisoryMode::Optional,
        observed_actual: None,
    };
    let route = |id: &str, enabled| ModelRoute {
        id: id.into(),
        provider: "configured-provider".into(),
        model: "configured-model".into(),
        effort: "configured-effort".into(),
        enabled,
        allowed_matrix_choice_ids: vec!["choice-a".into()],
        allowed_roles: vec!["agent".into()],
        allowed_tools: vec!["code".into()],
        allowed_data_classes: vec!["internal".into()],
        required_host_capabilities: vec!["model-api".into()],
        minimum_budget_units: 10,
        minimum_latency_ms: 50,
    };
    let catalogue = Catalogue(Some(ModelRouteCatalogue {
        schema: MODEL_ROUTE_CATALOGUE_SCHEMA.into(),
        version: 1,
        routes: vec![
            route("route-eligible", true),
            route("route-disabled", false),
        ],
    }));
    (
        request,
        Store {
            basis: Some(basis),
            ..Store::default()
        },
        catalogue,
    )
}

#[tokio::test]
async fn prepare_keeps_requested_ineligible_and_actual_independent() {
    let (request, mut store, catalogue) = fixture();
    store.basis.as_mut().unwrap().observed_actual = Some(ObservedModelRoute {
        route_id: Some("historical-unknown".into()),
        provider: "observed-provider".into(),
        model: "observed-model".into(),
        effort: "observed-effort".into(),
        evidence_ref: "execution/1".into(),
    });
    let prepared = request.prepare(&mut store, &catalogue).await.unwrap();
    assert_eq!(prepared.preparation, ModelRoutePreparation::Prepared);
    assert_eq!(
        prepared.eligible.as_ref().unwrap().route_ids,
        ["route-eligible"]
    );
    assert_eq!(
        prepared.routes.requested_route_id.as_deref(),
        Some("route-disabled")
    );
    assert_eq!(prepared.routes.recommended_route_id, None);
    assert_eq!(
        prepared
            .routes
            .observed_actual
            .as_ref()
            .unwrap()
            .route_id
            .as_deref(),
        Some("historical-unknown")
    );
    assert_eq!(store.captures, 1);
    assert_eq!(
        request.prepare(&mut store, &catalogue).await.unwrap(),
        prepared
    );
    assert_eq!(store.captures, 1);
}

#[tokio::test]
async fn unknown_request_and_stale_selection_are_rejected() {
    let (mut request, mut store, catalogue) = fixture();
    request.requested_route_id = Some("unknown".into());
    assert_eq!(
        request.prepare(&mut store, &catalogue).await,
        Err(Error::InvalidArguments)
    );
    assert_eq!(store.captures, 0);
    request.requested_route_id = None;
    request.expected_task_revision += 1;
    assert_eq!(
        request.prepare(&mut store, &catalogue).await,
        Err(Error::StaleContext)
    );
}

#[tokio::test]
async fn no_call_outcomes_are_durable_and_never_dispatch() {
    let cases = [
        (0, ModelRoutePreparation::WorkspaceDisabled),
        (1, ModelRoutePreparation::SessionSkip),
        (2, ModelRoutePreparation::RequestSkip),
        (3, ModelRoutePreparation::CapabilityUnavailable),
        (4, ModelRoutePreparation::NoEligibleRoutes),
    ];
    for (case, expected) in cases {
        let (mut request, mut store, mut catalogue) = fixture();
        request.requested_route_id = None;
        match case {
            0 => store.basis.as_mut().unwrap().advisory_mode = WorkspaceAdvisoryMode::Disabled,
            1 => request.session_preference = AdvisoryRequestPreference::Skip,
            2 => request.request_preference = AdvisoryRequestPreference::Skip,
            3 => catalogue.0 = None,
            _ => store.basis.as_mut().unwrap().work.remaining_budget_units = 0,
        }
        let prepared = request.prepare(&mut store, &catalogue).await.unwrap();
        assert_eq!(prepared.preparation, expected);
        assert_eq!(prepared.routes.recommended_route_id, None);
        assert_eq!(prepared.routes.observed_actual, None);
        assert_eq!(store.captures, 1);
    }
}
