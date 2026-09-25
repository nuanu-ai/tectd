use super::*;
use tect_application::{DisabledModelRouteRankingProvider, PreparedModelRouteRecommendation};

async fn prepare_case(
    store: &PgStore,
    runtime_pool: &PgPool,
    created: &super::super::positive::Fixture,
    label: &str,
) -> PreparedModelRouteRecommendation {
    let request = PrepareModelRouteRecommendation {
        workspace_id: created.workspace,
        disposition_id: created.selection.disposition_id,
        expected_task_id: created.task,
        expected_task_revision: created.selection.task_revision,
        expected_candidate_set_id: created.candidate_set,
        expected_caller_request_id: created.caller_request,
        expected_mapped_work_node_id: created.work_node,
        expected_mapped_work_node_revision: created.work_revision,
        request_key: format!("route-{label}-{}", Uuid::new_v4()),
        requested_route_id: None,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
    };
    let mut writer = store.begin(TransactionMode::ReadWrite).await.unwrap();
    writer.authenticate(&created.owner.auth).await.unwrap();
    writer.set_tenant(created.tenant).await.unwrap();
    let mut reader = PgUnitOfWork::test_begin(runtime_pool, created.tenant).await;
    let prepared = request
        .prepare(
            writer.model_route_recommendation_store().unwrap(),
            &mut reader,
            &TestHost,
            &TestCatalogue,
        )
        .await
        .unwrap();
    writer.commit().await.unwrap();
    prepared
}

async fn writer(
    store: &PgStore,
    created: &super::super::positive::Fixture,
) -> Box<dyn tect_application::UnitOfWork> {
    let mut writer = store.begin(TransactionMode::ReadWrite).await.unwrap();
    writer.authenticate(&created.owner.auth).await.unwrap();
    writer.set_tenant(created.tenant).await.unwrap();
    writer
}

#[tokio::test]
#[ignore = "requires identity-pinned disposable PG18 and TECT_TEST_* URLs"]
async fn optional_ranker_no_call_unknown_send_and_malformed_raw_are_audited_live() {
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
    let store = PgStore::from_pool(runtime_pool.clone());
    let invocation = ModelRouteInvocation {
        session_id: created.invocation_session,
    };
    let calls = Arc::new(AtomicUsize::new(0));

    let disabled = prepare_case(&store, &runtime_pool, &created, "disabled").await;
    let mut tx = writer(&store, &created).await;
    assert_eq!(
        prepare_model_route_send(
            tx.model_route_attempt_store().unwrap(),
            &DisabledModelRouteRankingProvider,
            &disabled,
            invocation
        )
        .await
        .unwrap(),
        ModelRouteSendStart::NoCall(tect_application::ModelRouteRunNoCall::ProviderUnavailable)
    );
    tx.commit().await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let unknown = prepare_case(&store, &runtime_pool, &created, "unknown-send").await;
    let ranker = FakeJevRanker {
        pool: runtime_pool.clone(),
        tenant: created.tenant,
        calls: calls.clone(),
        malformed: false,
    };
    let mut tx = writer(&store, &created).await;
    let (attempted, permit) = match prepare_model_route_send(
        tx.model_route_attempt_store().unwrap(),
        &ranker,
        &unknown,
        invocation,
    )
    .await
    .unwrap()
    {
        ModelRouteSendStart::Started { attempted, permit } => (attempted, permit),
        other => panic!("expected guarded send: {other:?}"),
    };
    tx.commit().await.unwrap();
    // A crash/unknown transport result after committed authorization never
    // creates a second permit, even without a sealed response.
    let mut tx = writer(&store, &created).await;
    tx.model_route_attempt_store()
        .unwrap()
        .mark_send_unknown(&permit)
        .await
        .unwrap();
    assert_eq!(
        prepare_model_route_send(
            tx.model_route_attempt_store().unwrap(),
            &ranker,
            &unknown,
            invocation
        )
        .await
        .unwrap(),
        ModelRouteSendStart::Replay
    );
    tx.commit().await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let malformed = prepare_case(&store, &runtime_pool, &created, "malformed").await;
    let malformed_ranker = FakeJevRanker {
        pool: runtime_pool.clone(),
        tenant: created.tenant,
        calls: calls.clone(),
        malformed: true,
    };
    let mut tx = writer(&store, &created).await;
    let (bad_attempted, bad_permit) = match prepare_model_route_send(
        tx.model_route_attempt_store().unwrap(),
        &malformed_ranker,
        &malformed,
        invocation,
    )
    .await
    .unwrap()
    {
        ModelRouteSendStart::Started { attempted, permit } => (attempted, permit),
        other => panic!("expected guarded malformed send: {other:?}"),
    };
    let bad_raw = attempt_model_route_after_commit(
        tx.commit(),
        &malformed_ranker,
        bad_attempted.clone(),
        bad_permit.clone(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let mut tx = writer(&store, &created).await;
    let bad_digest = seal_model_route_raw_response(
        tx.model_route_attempt_store().unwrap(),
        &bad_permit,
        &bad_raw,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let mut tx = writer(&store, &created).await;
    assert!(
        finalize_model_route_sealed_response(
            tx.model_route_attempt_store().unwrap(),
            &malformed,
            &bad_attempted,
            &bad_permit
        )
        .await
        .is_err()
    );
    drop(tx); // rollback the failed parse transaction; the prior raw seal remains committed.
    let mut tx = writer(&store, &created).await;
    assert_eq!(
        prepare_model_route_send(
            tx.model_route_attempt_store().unwrap(),
            &malformed_ranker,
            &malformed,
            invocation
        )
        .await
        .unwrap(),
        ModelRouteSendStart::Replay
    );
    tx.commit().await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let mut audit = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(created.tenant.to_string())
        .execute(&mut *audit)
        .await
        .unwrap();
    let rows = sqlx::query("SELECT preparation_request_key,state,no_call_reason,request_payload,response_payload,response_sha256 FROM model_route_advisory_attempts WHERE tenant_id=$1 AND workspace_id=$2")
        .bind(created.tenant).bind(created.workspace).fetch_all(&mut *audit).await.unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().any(
        |r| r.try_get::<String, _>("preparation_request_key").unwrap() == disabled.request_key
            && r.try_get::<String, _>("state").unwrap() == "no_call"
            && r.try_get::<Option<String>, _>("no_call_reason").unwrap()
                == Some("provider_unavailable".into())
    ));
    assert!(rows.iter().any(
        |r| r.try_get::<String, _>("preparation_request_key").unwrap() == unknown.request_key
            && r.try_get::<String, _>("state").unwrap() == "send_unknown"
            && r.try_get::<Option<Vec<u8>>, _>("request_payload").unwrap()
                == Some(attempted.request_bytes.clone())
    ));
    assert!(rows.iter().any(
        |r| r.try_get::<String, _>("preparation_request_key").unwrap() == malformed.request_key
            && r.try_get::<String, _>("state").unwrap() == "raw_sealed"
            && r.try_get::<Option<Vec<u8>>, _>("response_payload").unwrap()
                == Some(bad_raw.clone())
            && r.try_get::<Option<String>, _>("response_sha256").unwrap()
                == Some(bad_digest.clone())
    ));
    let counts = sqlx::query("SELECT count(*) AS rows,sum(call_count) AS calls,bool_and(step='recommendation_before_model_choice') AS step_ok FROM advisory_call_audit WHERE tenant_id=$1 AND workspace_id=$2 AND capability='model_routing'")
        .bind(created.tenant).bind(created.workspace).fetch_one(&mut *audit).await.unwrap();
    assert_eq!(counts.try_get::<i64, _>("rows").unwrap(), 3);
    assert_eq!(counts.try_get::<i64, _>("calls").unwrap(), 2);
    assert!(counts.try_get::<bool, _>("step_ok").unwrap());
    audit.rollback().await.unwrap();
}
