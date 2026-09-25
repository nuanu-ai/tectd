use super::*;
use crate::{
    ModelRouteCatalogueProvider, ModelRouteHostCapabilitiesProvider, ModelRouteRecommendationBasis,
    ModelRouteRecommendationStore, ModelRouteSelectionRead, PreparedModelRouteRecommendation,
};
use async_trait::async_trait;
use tect_domain::{
    MODEL_ROUTE_CATALOGUE_SCHEMA, MatrixPlanningSelection, ModelRoute, ModelRouteCatalogue,
    ModelRouteFact, ModelRouteFactProvenance, ModelRouteSelectionLink, ModelRouteWorkContext,
};

struct Catalogue(Option<ModelRouteCatalogue>);

struct Reader(Option<ModelRouteWorkContext>);

#[async_trait]
impl ModelRouteSelectionRead for Reader {
    async fn approved_work_context(
        &mut self,
        _workspace_id: Uuid,
        _disposition_id: Uuid,
        _candidate_set_id: Uuid,
        _caller_request_id: Uuid,
        _mapped_work_node_id: Uuid,
        _mapped_work_node_revision: i64,
    ) -> Result<Option<ModelRouteWorkContext>> {
        Ok(self.0.clone())
    }
}

struct Host(ModelRouteFact<Vec<String>>);

impl ModelRouteHostCapabilitiesProvider for Host {
    fn host_capabilities(&self) -> Result<ModelRouteFact<Vec<String>>> {
        Ok(self.0.clone())
    }
}

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

fn fixture() -> (
    PrepareModelRouteRecommendation,
    Store,
    Reader,
    Host,
    Catalogue,
) {
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
        expected_candidate_set_id: Uuid::new_v4(),
        expected_caller_request_id: Uuid::new_v4(),
        expected_mapped_work_node_id: Uuid::from_u128(2),
        expected_mapped_work_node_revision: 2,
        request_key: "request-1".into(),
        requested_route_id: Some("route-disabled".into()),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
    };
    let work = ModelRouteWorkContext {
        approved_matrix_selection: selection,
        selection_link: ModelRouteSelectionLink {
            candidate_set_id: request.expected_candidate_set_id,
            caller_request_id: request.expected_caller_request_id,
            mapped_draft_node_index: 0,
            mapped_work_node_id: request.expected_mapped_work_node_id,
            mapped_work_node_revision: request.expected_mapped_work_node_revision,
        },
        role: caller("agent".into()),
        tool: caller("code".into()),
        data_class: caller("internal".into()),
        host_capabilities: ModelRouteFact::Unknown,
        remaining_budget_units: caller(20),
        available_latency_ms: caller(100),
    };
    let basis = ModelRouteRecommendationBasis {
        advisory_mode: WorkspaceAdvisoryMode::Optional,
    };
    let host = Host(ModelRouteFact::Known {
        value: vec!["model-api".into()],
        provenance: ModelRouteFactProvenance::Host {
            evidence_ref: "host/caps/1".into(),
        },
    });
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
        Reader(Some(work)),
        host,
        catalogue,
    )
}

fn caller<T>(value: T) -> ModelRouteFact<T> {
    ModelRouteFact::Known {
        value,
        provenance: ModelRouteFactProvenance::Caller {
            source_ref: "work/node/2".into(),
            work_node_id: Uuid::from_u128(2),
            work_node_revision: 2,
        },
    }
}

#[tokio::test]
async fn prepare_keeps_requested_ineligible_and_actual_unknown_without_execution_evidence() {
    let (request, mut store, mut reader, host, catalogue) = fixture();
    let prepared = request
        .prepare(&mut store, &mut reader, &host, &catalogue)
        .await
        .unwrap();
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
    assert_eq!(prepared.routes.observed_actual, None);
    assert_eq!(store.captures, 1);
    assert_eq!(
        request
            .prepare(&mut store, &mut reader, &host, &catalogue)
            .await
            .unwrap(),
        prepared
    );
    assert_eq!(store.captures, 1);
}

#[tokio::test]
async fn unknown_request_and_stale_selection_are_rejected() {
    let (mut request, mut store, mut reader, host, catalogue) = fixture();
    request.requested_route_id = Some("unknown".into());
    assert_eq!(
        request
            .prepare(&mut store, &mut reader, &host, &catalogue)
            .await,
        Err(Error::InvalidArguments)
    );
    assert_eq!(store.captures, 0);
    request.requested_route_id = None;
    request.expected_task_revision += 1;
    assert_eq!(
        request
            .prepare(&mut store, &mut reader, &host, &catalogue)
            .await,
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
        let (mut request, mut store, mut reader, host, mut catalogue) = fixture();
        request.requested_route_id = None;
        match case {
            0 => store.basis.as_mut().unwrap().advisory_mode = WorkspaceAdvisoryMode::Disabled,
            1 => request.session_preference = AdvisoryRequestPreference::Skip,
            2 => request.request_preference = AdvisoryRequestPreference::Skip,
            3 => catalogue.0 = None,
            _ => reader.0.as_mut().unwrap().remaining_budget_units = caller(0),
        }
        let prepared = request
            .prepare(&mut store, &mut reader, &host, &catalogue)
            .await
            .unwrap();
        assert_eq!(prepared.preparation, expected);
        assert_eq!(prepared.routes.recommended_route_id, None);
        assert_eq!(prepared.routes.observed_actual, None);
        assert_eq!(store.captures, 1);
    }
}

#[tokio::test]
async fn exact_saved_link_and_unknown_facts_gate_preparation() {
    let (mut request, mut store, mut reader, host, catalogue) = fixture();
    request.expected_caller_request_id = Uuid::new_v4();
    assert_eq!(
        request
            .prepare(&mut store, &mut reader, &host, &catalogue)
            .await,
        Err(Error::StaleContext)
    );
    assert_eq!(store.captures, 0);
    request.expected_caller_request_id =
        reader.0.as_ref().unwrap().selection_link.caller_request_id;
    reader.0.as_mut().unwrap().role = ModelRouteFact::Unknown;
    let prepared = request
        .prepare(&mut store, &mut reader, &host, &catalogue)
        .await
        .unwrap();
    assert_eq!(
        prepared.preparation,
        ModelRoutePreparation::UnknownWorkFacts
    );
    assert!(prepared.eligible.unwrap().route_ids.is_empty());
}

#[tokio::test]
async fn absent_host_capability_evidence_is_unknown_even_with_typed_work() {
    let (mut request, mut store, mut reader, mut host, catalogue) = fixture();
    request.requested_route_id = None;
    host.0 = ModelRouteFact::Unknown;
    let prepared = request
        .prepare(&mut store, &mut reader, &host, &catalogue)
        .await
        .unwrap();
    assert_eq!(
        prepared.preparation,
        ModelRoutePreparation::UnknownWorkFacts
    );
    assert!(prepared.eligible.unwrap().route_ids.is_empty());
    assert_eq!(prepared.routes.observed_actual, None);
}
