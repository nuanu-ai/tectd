use super::*;
use crate::model_route_live_tests::positive::UnusedAdapters;
use tect_application::WorkspaceService;

fn request(
    created: &super::super::positive::Fixture,
    label: &str,
) -> PrepareModelRouteRecommendation {
    PrepareModelRouteRecommendation {
        workspace_id: Uuid::nil(), // WorkspaceService derives this from auth.
        disposition_id: created.selection.disposition_id,
        expected_task_id: created.task,
        expected_task_revision: created.selection.task_revision,
        expected_candidate_set_id: created.candidate_set,
        expected_caller_request_id: created.caller_request,
        expected_mapped_work_node_id: created.work_node,
        expected_mapped_work_node_revision: created.work_revision,
        request_key: format!("public-route-{label}-{}", Uuid::new_v4()),
        requested_route_id: None,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
    }
}

#[tokio::test]
#[ignore = "requires identity-pinned disposable PG18 and TECT_TEST_* URLs"]
async fn service_public_route_uses_one_fake_call_and_disabled_no_call() {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_pool = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    let runtime_pool = PgPool::connect(&std::env::var("TECT_TEST_RUNTIME_URL").unwrap())
        .await
        .unwrap();
    let identity: (String, i64, String) = sqlx::query_as(
        "SELECT current_database(),(SELECT oid::bigint FROM pg_catalog.pg_database WHERE datname=current_database()),(SELECT system_identifier::text FROM pg_catalog.pg_control_system())"
    ).fetch_one(&admin_pool).await.unwrap();
    assert_eq!(
        (identity.0.as_str(), identity.1, identity.2.as_str()),
        ("tect_test", 16385, "7689349823162929726")
    );
    let ledger: i64 = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(&admin_pool)
        .await
        .unwrap();
    assert_eq!(ledger, 82);
    let created = fixture(&admin_pool, &runtime_pool).await;
    let context = tect_domain::RequestContext {
        auth: created.owner.auth.clone(),
        native_session_id: created.invocation_session.to_string(),
        workspace_key: format!("route-positive-{}", created.workspace),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let fake = Arc::new(FakeJevRanker {
        pool: runtime_pool.clone(),
        tenant: created.tenant,
        calls: calls.clone(),
        malformed: false,
    });
    let adapters = Arc::new(UnusedAdapters);
    let service = WorkspaceService::new(
        Arc::new(PgStore::from_pool(runtime_pool.clone())),
        adapters.clone(),
        adapters.clone(),
    )
    .with_model_route_catalogue_provider(Arc::new(TestCatalogue))
    .with_model_route_host_capabilities_provider(Arc::new(TestHost))
    .with_model_route_ranking_provider(fake);

    let prepare = request(&created, "rank");
    let saved = service
        .prepare_model_route(&context, &prepare)
        .await
        .unwrap();
    assert_eq!(saved.preparation, ModelRoutePreparation::Prepared);
    assert_eq!(saved.workspace_id, created.workspace);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let view = service
        .run_model_route(&context, &prepare.request_key)
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        view.attempt.as_ref().unwrap().state,
        tect_application::ModelRouteAttemptState::Parsed
    );
    let decision = view.decision.as_ref().unwrap();
    assert_eq!(
        decision.outcome,
        ModelRouteDecisionOutcome::Recommended {
            route_id: "route-a".into()
        }
    );
    assert_eq!(decision.routes.requested_route_id, None);
    assert_eq!(
        decision.routes.recommended_route_id.as_deref(),
        Some("route-a")
    );
    assert_eq!(decision.routes.observed_actual, None);
    assert_eq!(
        service
            .run_model_route(&context, &prepare.request_key)
            .await
            .unwrap(),
        view
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let disposition_id = Uuid::new_v4();
    let accepted = service
        .disposition_model_route(
            &context,
            decision.id,
            disposition_id,
            ModelRouteDispositionAction::Accept,
            "Synthetic acceptance only".into(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.action, ModelRouteDispositionAction::Accept);
    assert_eq!(
        service
            .disposition_model_route(
                &context,
                decision.id,
                disposition_id,
                ModelRouteDispositionAction::Accept,
                "Synthetic acceptance only".into()
            )
            .await
            .unwrap(),
        accepted
    );
    assert!(matches!(
        service
            .disposition_model_route(
                &context,
                decision.id,
                disposition_id,
                ModelRouteDispositionAction::Reject,
                "Changed".into()
            )
            .await,
        Err(Error::InputConflict)
    ));
    assert_eq!(
        service
            .get_model_route(&context, &prepare.request_key)
            .await
            .unwrap()
            .disposition,
        Some(accepted)
    );

    let disabled = WorkspaceService::new(
        Arc::new(PgStore::from_pool(runtime_pool.clone())),
        adapters.clone(),
        adapters,
    )
    .with_model_route_catalogue_provider(Arc::new(TestCatalogue))
    .with_model_route_host_capabilities_provider(Arc::new(TestHost));
    let no_call_request = request(&created, "disabled");
    assert_eq!(
        disabled
            .prepare_model_route(&context, &no_call_request)
            .await
            .unwrap()
            .preparation,
        ModelRoutePreparation::Prepared
    );
    let no_call = disabled
        .run_model_route(&context, &no_call_request.request_key)
        .await
        .unwrap();
    assert_eq!(
        no_call.attempt.unwrap().state,
        tect_application::ModelRouteAttemptState::NoCall
    );
    assert!(matches!(
        no_call.decision.unwrap().outcome,
        ModelRouteDecisionOutcome::Abstained {
            reason: tect_application::ModelRouteAbstainReason::NoCall
        }
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
