use super::*;
use crate::PgStore;
use crate::model_route_live_tests::positive::fixture;
use tect_application::{
    DecideModelRouteRecommendation, DispositionModelRouteRecommendation,
    ModelRouteCatalogueProvider, ModelRouteDecisionInput, ModelRouteDecisionOutcome,
    ModelRouteDispositionAction, ModelRouteHostCapabilitiesProvider, ModelRoutePreparation,
    ModelRouteSelectionRead, PrepareModelRouteRecommendation, Store, TransactionMode,
};
use tect_domain::{
    AdvisoryRequestPreference, MODEL_ROUTE_CATALOGUE_SCHEMA, MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA,
    ModelRoute, ModelRouteCatalogue, ModelRouteFactProvenance, ModelRouteHostCapabilities,
    ModelRouteRanking,
};

struct TestCatalogue;
impl ModelRouteCatalogueProvider for TestCatalogue {
    fn catalogue(&self) -> tect_domain::Result<Option<ModelRouteCatalogue>> {
        Ok(Some(ModelRouteCatalogue {
            schema: MODEL_ROUTE_CATALOGUE_SCHEMA.into(),
            version: 1,
            routes: vec![ModelRoute {
                id: "route-a".into(),
                provider: "configured-provider".into(),
                model: "configured-model".into(),
                effort: "medium".into(),
                enabled: true,
                allowed_matrix_choice_ids: vec!["choice-a".into()],
                allowed_roles: vec!["agent".into()],
                allowed_tools: vec!["code".into()],
                allowed_data_classes: vec!["internal".into()],
                required_host_capabilities: vec!["model-api".into()],
                minimum_budget_units: 10,
                minimum_latency_ms: 50,
            }],
        }))
    }
}

struct TestHost;
impl ModelRouteHostCapabilitiesProvider for TestHost {
    fn host_capabilities(&self) -> tect_domain::Result<ModelRouteFact<Vec<String>>> {
        ModelRouteHostCapabilities {
            schema: MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
            version: 1,
            capabilities: vec!["model-api".into()],
        }
        .fact()
    }
}

#[tokio::test]
#[ignore = "requires identity-pinned disposable PG18 and TECT_TEST_* URLs"]
async fn current_authorized_matrix_selected_work_save_is_readable() {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_pool = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    let runtime_pool = PgPool::connect(&std::env::var("TECT_TEST_RUNTIME_URL").unwrap())
        .await
        .unwrap();
    let identity: (String,i64,String) = sqlx::query_as(
        "SELECT current_database(),(SELECT oid::bigint FROM pg_catalog.pg_database WHERE datname=current_database()),(SELECT system_identifier::text FROM pg_catalog.pg_control_system())"
    ).fetch_one(&admin_pool).await.unwrap();
    assert_eq!(identity.0, "tect_test");
    assert_eq!(
        identity.1.to_string(),
        std::env::var("TECT_TEST_EXPECTED_DB_OID").unwrap()
    );
    assert_eq!(
        identity.2,
        std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").unwrap()
    );
    let created = fixture(&admin_pool, &runtime_pool).await;
    let mut reader = PgUnitOfWork::test_begin(&runtime_pool, created.tenant).await;
    let work = reader
        .approved_work_context(
            created.workspace,
            created.selection.disposition_id,
            created.candidate_set,
            created.caller_request,
            created.work_node,
            created.work_revision,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(work.selection_link.mapped_work_node_id, created.work_node);
    for fact in [&work.role, &work.tool, &work.data_class] {
        assert!(
            matches!(fact, ModelRouteFact::Known { provenance: ModelRouteFactProvenance::Caller { work_node_id, work_node_revision, .. }, .. } if *work_node_id == created.work_node && *work_node_revision == created.work_revision)
        );
    }
    assert!(matches!(
        work.remaining_budget_units,
        ModelRouteFact::Known {
            provenance: ModelRouteFactProvenance::Caller { .. },
            value: 20
        }
    ));
    assert!(matches!(
        work.available_latency_ms,
        ModelRouteFact::Known {
            provenance: ModelRouteFactProvenance::Caller { .. },
            value: 100
        }
    ));
    assert!(matches!(work.host_capabilities, ModelRouteFact::Unknown));
    drop(reader);

    let store = PgStore::from_pool(runtime_pool.clone());
    let request = PrepareModelRouteRecommendation {
        workspace_id: created.workspace,
        disposition_id: created.selection.disposition_id,
        expected_task_id: created.task,
        expected_task_revision: created.selection.task_revision,
        expected_candidate_set_id: created.candidate_set,
        expected_caller_request_id: created.caller_request,
        expected_mapped_work_node_id: created.work_node,
        expected_mapped_work_node_revision: created.work_revision,
        request_key: format!("route-positive-{}", Uuid::new_v4()),
        requested_route_id: None,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
    };
    let mut writer = store.begin(TransactionMode::ReadWrite).await.unwrap();
    writer.authenticate(&created.owner.auth).await.unwrap();
    writer.set_tenant(created.tenant).await.unwrap();
    let mut reader = PgUnitOfWork::test_begin(&runtime_pool, created.tenant).await;
    let prepared = request
        .prepare(
            writer.model_route_recommendation_store().unwrap(),
            &mut reader,
            &TestHost,
            &TestCatalogue,
        )
        .await
        .unwrap();
    assert_eq!(prepared.preparation, ModelRoutePreparation::Prepared);
    assert_eq!(prepared.eligible.as_ref().unwrap().route_ids, ["route-a"]);
    assert_eq!(prepared.routes.observed_actual, None);
    writer.commit().await.unwrap();

    let ranking = ModelRouteRanking {
        catalogue_digest: prepared.eligible.as_ref().unwrap().catalogue_digest.clone(),
        work_context_digest: prepared
            .eligible
            .as_ref()
            .unwrap()
            .work_context_digest
            .clone(),
        ranked_route_ids: vec!["route-a".into()],
    };
    let decision = DecideModelRouteRecommendation {
        id: Uuid::new_v4(),
        workspace_id: created.workspace,
        preparation_request_key: request.request_key.clone(),
        input: ModelRouteDecisionInput::Ranking(ranking),
    };
    let mut prep_read = PgUnitOfWork::test_begin(&runtime_pool, created.tenant).await;
    let mut decision_write = store.begin(TransactionMode::ReadWrite).await.unwrap();
    decision_write
        .authenticate(&created.owner.auth)
        .await
        .unwrap();
    decision_write.set_tenant(created.tenant).await.unwrap();
    let captured = decision
        .decide(
            &mut prep_read,
            decision_write.model_route_decision_store().unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        captured.outcome,
        ModelRouteDecisionOutcome::Recommended {
            route_id: "route-a".into()
        }
    );
    assert_eq!(
        captured.routes.recommended_route_id.as_deref(),
        Some("route-a")
    );
    assert_eq!(captured.routes.observed_actual, None);
    decision_write.commit().await.unwrap();

    let disposition = DispositionModelRouteRecommendation {
        id: Uuid::new_v4(),
        workspace_id: created.workspace,
        decision_id: decision.id,
        actor_id: created.owner.principal_id,
        action: ModelRouteDispositionAction::Accept,
        rationale: "Synthetic test acceptance; no dispatch".into(),
    };
    let mut final_write = store.begin(TransactionMode::ReadWrite).await.unwrap();
    final_write.authenticate(&created.owner.auth).await.unwrap();
    final_write.set_tenant(created.tenant).await.unwrap();
    let accepted = disposition
        .record(final_write.model_route_decision_store().unwrap())
        .await
        .unwrap();
    assert_eq!(accepted.action, ModelRouteDispositionAction::Accept);
    final_write.commit().await.unwrap();

    let mut replay = store.begin(TransactionMode::ReadWrite).await.unwrap();
    replay.authenticate(&created.owner.auth).await.unwrap();
    replay.set_tenant(created.tenant).await.unwrap();
    assert_eq!(
        disposition
            .record(replay.model_route_decision_store().unwrap())
            .await
            .unwrap(),
        accepted
    );
    let conflict = DispositionModelRouteRecommendation {
        action: ModelRouteDispositionAction::Reject,
        ..disposition
    };
    assert!(matches!(
        conflict
            .record(replay.model_route_decision_store().unwrap())
            .await,
        Err(Error::InputConflict)
    ));
    replay.commit().await.unwrap();
}
