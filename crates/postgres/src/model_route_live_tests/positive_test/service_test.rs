use super::*;
use crate::model_route_live_tests::positive::UnusedAdapters;
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::os::unix::fs::PermissionsExt;
use tect_application::WorkspaceService;
use tokio::net::UnixListener;

struct RevokeAfterCommittedSend {
    inner: FakeJevRanker,
    admin_pool: PgPool,
    session_id: Uuid,
}

#[async_trait]
impl ModelRouteRankingProvider for RevokeAfterCommittedSend {
    fn prepare(
        &self,
        saved: &tect_application::PreparedModelRouteRecommendation,
    ) -> tect_domain::Result<ModelRoutePreparedAttempt> {
        self.inner.prepare(saved)
    }

    async fn attempt_prepared(
        &self,
        attempted: ModelRoutePreparedAttempt,
        permit: ModelRouteSendPermit,
    ) -> tect_domain::Result<Vec<u8>> {
        let raw = self.inner.attempt_prepared(attempted, permit).await?;
        crate::admin::revoke_session(&self.admin_pool, self.session_id).await?;
        Ok(raw)
    }
}

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
async fn service_fake_provider_has_zero_calls_without_trusted_policy() {
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
        identity.0,
        std::env::var("TECT_TEST_EXPECTED_DB_NAME").unwrap()
    );
    assert_eq!(
        identity.1.to_string(),
        std::env::var("TECT_TEST_EXPECTED_DB_OID").unwrap()
    );
    assert_eq!(
        identity.2,
        std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").unwrap()
    );
    crate::admin::migrate(
        &admin_pool,
        &std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap(),
    )
    .await
    .unwrap();
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
        adapters,
    )
    .with_model_route_catalogue_provider(Arc::new(TestCatalogue))
    .with_model_route_host_capabilities_provider(Arc::new(TestHost))
    .with_model_route_ranking_provider(fake);
    let prepare = request(&created, "no-trusted-policy");
    service
        .prepare_model_route(&context, &prepare)
        .await
        .unwrap();
    assert!(matches!(
        service
            .run_model_route(&context, &prepare.request_key)
            .await,
        Err(Error::BudgetPolicyInvalid)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let view = service
        .get_model_route(&context, &prepare.request_key)
        .await
        .unwrap();
    assert!(view.attempt.is_none());
    assert!(view.decision.is_none());
    assert!(view.disposition.is_none());
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

#[tokio::test]
#[ignore = "requires identity-pinned disposable PG18 and TECT_TEST_* URLs"]
async fn unix_host_model_route_tools_preserve_the_committed_fake_call() {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_pool = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    let runtime_pool = PgPool::connect(&std::env::var("TECT_TEST_RUNTIME_URL").unwrap())
        .await
        .unwrap();
    let identity: (String, i64, String, i64) = sqlx::query_as(
        "SELECT current_database(),(SELECT oid::bigint FROM pg_catalog.pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_catalog.pg_control_system()),\
         (SELECT max(version) FROM _sqlx_migrations)",
    )
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert_eq!(
        (
            identity.0.as_str(),
            identity.1,
            identity.2.as_str(),
            identity.3
        ),
        ("tect_test", 16385, "7689349823162929726", 82)
    );
    let created = fixture(&admin_pool, &runtime_pool).await;
    let context = tect_domain::RequestContext {
        auth: created.owner.auth.clone(),
        native_session_id: created.invocation_session.to_string(),
        workspace_key: format!("route-positive-{}", created.workspace),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let adapters = Arc::new(UnusedAdapters);
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(PgStore::from_pool(runtime_pool.clone())),
            adapters.clone(),
            adapters,
        )
        .with_model_route_catalogue_provider(Arc::new(TestCatalogue))
        .with_model_route_host_capabilities_provider(Arc::new(TestHost))
        .with_model_route_ranking_provider(Arc::new(FakeJevRanker {
            pool: runtime_pool.clone(),
            tenant: created.tenant,
            calls: calls.clone(),
            malformed: false,
        })),
    );
    let socket_dir = std::path::Path::new("/private/tmp").join(format!("tr-{}", Uuid::new_v4()));
    std::fs::create_dir(&socket_dir).unwrap();
    std::fs::set_permissions(&socket_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = socket_dir.join("host.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));

    let prepared = request(&created, "unix");
    let args = serde_json::json!({
        "disposition_id":prepared.disposition_id,
        "expected_task_id":prepared.expected_task_id,
        "expected_task_revision":prepared.expected_task_revision,
        "expected_candidate_set_id":prepared.expected_candidate_set_id,
        "expected_caller_request_id":prepared.expected_caller_request_id,
        "expected_mapped_work_node_id":prepared.expected_mapped_work_node_id,
        "expected_mapped_work_node_revision":prepared.expected_mapped_work_node_revision,
        "request_key":prepared.request_key,
    });
    let mut invalid_null = args.clone();
    invalid_null["requested_route_id"] = serde_json::Value::Null;
    assert!(matches!(
        tect_host::call_tool(&socket, &context, "model_route_prepare", invalid_null).await,
        Err(Error::InvalidArguments)
    ));
    let saved = tect_host::call_tool(&socket, &context, "model_route_prepare", args)
        .await
        .unwrap();
    assert_eq!(saved["preparation"], "Prepared");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let key = serde_json::json!({"preparation_request_key":prepared.request_key});
    assert!(matches!(
        tect_host::call_tool(
            &socket,
            &context,
            "model_route_run",
            serde_json::json!({"preparation_request_key":prepared.request_key,
                "ranked_route_ids":["route-a"]}),
        )
        .await,
        Err(Error::InvalidArguments)
    ));
    let view = tect_host::call_tool(&socket, &context, "model_route_run", key.clone())
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(view["attempt"]["state"], "parsed");
    assert_eq!(
        view["decision"]["routes"]["recommended_route_id"],
        "route-a"
    );
    assert!(view["decision"]["routes"]["requested_route_id"].is_null());
    assert!(view["decision"]["routes"]["observed_actual"].is_null());
    assert_eq!(
        tect_host::call_tool(&socket, &context, "model_route_run", key.clone())
            .await
            .unwrap(),
        view
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let read = tect_host::call_tool(&socket, &context, "model_route_get", key.clone())
        .await
        .unwrap();
    assert_eq!(read, view);
    let disposition_id = Uuid::new_v4();
    let disposition_args = serde_json::json!({
        "disposition_id":disposition_id,
        "decision_id":view["decision"]["id"],
        "action":"accept",
        "rationale":"Synthetic acceptance only",
    });
    let disposition = tect_host::call_tool(
        &socket,
        &context,
        "model_route_disposition",
        disposition_args.clone(),
    )
    .await
    .unwrap();
    assert_eq!(disposition["action"], "Accept");
    assert_eq!(
        tect_host::call_tool(
            &socket,
            &context,
            "model_route_disposition",
            disposition_args
        )
        .await
        .unwrap(),
        disposition
    );
    let after_disposition = tect_host::call_tool(&socket, &context, "model_route_get", key)
        .await
        .unwrap();
    assert_eq!(after_disposition["disposition"]["id"], disposition["id"]);
    assert_eq!(
        after_disposition["disposition"]["action"],
        disposition["action"]
    );
    let mut tenant_read = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(created.tenant.to_string())
        .execute(&mut *tenant_read)
        .await
        .unwrap();
    let row = sqlx::query(
        "SELECT state,request_payload,request_sha256,response_payload,response_sha256 \
         FROM model_route_advisory_attempts WHERE tenant_id=$1 AND workspace_id=$2 \
         AND preparation_request_key=$3",
    )
    .bind(created.tenant)
    .bind(created.workspace)
    .bind(&prepared.request_key)
    .fetch_one(&mut *tenant_read)
    .await
    .unwrap();
    assert_eq!(row.try_get::<String, _>("state").unwrap(), "parsed");
    for (payload, digest) in [
        ("request_payload", "request_sha256"),
        ("response_payload", "response_sha256"),
    ] {
        let bytes: Vec<u8> = row.try_get(payload).unwrap();
        let saved: String = row.try_get(digest).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), saved);
    }
    tenant_read.rollback().await.unwrap();
    server.abort();
    std::fs::remove_file(socket).unwrap();
    std::fs::remove_dir(socket_dir).unwrap();
}

#[tokio::test]
#[ignore = "requires identity-pinned disposable PG18 and TECT_TEST_* URLs"]
async fn revoked_session_after_send_still_seals_raw_without_decision() {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_pool = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    let runtime_pool = PgPool::connect(&std::env::var("TECT_TEST_RUNTIME_URL").unwrap())
        .await
        .unwrap();
    let identity: (String, i64, String, i64) = sqlx::query_as(
        "SELECT current_database(),(SELECT oid::bigint FROM pg_catalog.pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_catalog.pg_control_system()),\
         (SELECT max(version) FROM _sqlx_migrations)",
    )
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert_eq!(
        (
            identity.0.as_str(),
            identity.1,
            identity.2.as_str(),
            identity.3
        ),
        ("tect_test", 16385, "7689349823162929726", 82)
    );
    let created = fixture(&admin_pool, &runtime_pool).await;
    let context = tect_domain::RequestContext {
        auth: created.owner.auth.clone(),
        native_session_id: created.invocation_session.to_string(),
        workspace_key: format!("route-positive-{}", created.workspace),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let adapters = Arc::new(UnusedAdapters);
    let store = PgStore::from_pool(runtime_pool.clone());
    let service = WorkspaceService::new(Arc::new(store.clone()), adapters.clone(), adapters)
        .with_model_route_catalogue_provider(Arc::new(TestCatalogue))
        .with_model_route_host_capabilities_provider(Arc::new(TestHost))
        .with_model_route_ranking_provider(Arc::new(RevokeAfterCommittedSend {
            inner: FakeJevRanker {
                pool: runtime_pool.clone(),
                tenant: created.tenant,
                calls: calls.clone(),
                malformed: false,
            },
            admin_pool: admin_pool.clone(),
            session_id: created.invocation_session,
        }));
    let prepared = request(&created, "revoke-after-send");
    assert_eq!(
        service
            .prepare_model_route(&context, &prepared)
            .await
            .unwrap()
            .preparation,
        ModelRoutePreparation::Prepared
    );
    assert!(matches!(
        service
            .run_model_route(&context, &prepared.request_key)
            .await,
        Err(Error::SessionRevoked)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let mut tx = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(created.tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let row = sqlx::query(
        "SELECT id,state,request_sha256,response_payload,response_sha256 \
         FROM model_route_advisory_attempts WHERE tenant_id=$1 AND workspace_id=$2 \
         AND preparation_request_key=$3",
    )
    .bind(created.tenant)
    .bind(created.workspace)
    .bind(&prepared.request_key)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(row.try_get::<String, _>("state").unwrap(), "raw_sealed");
    let raw: Vec<u8> = row.try_get("response_payload").unwrap();
    let digest: String = row.try_get("response_sha256").unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(&raw)), digest);
    let decisions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM model_route_decisions WHERE tenant_id=$1 AND workspace_id=$2 \
         AND preparation_request_key=$3",
    )
    .bind(created.tenant)
    .bind(created.workspace)
    .bind(&prepared.request_key)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(decisions, 0);
    let wrong_permit = ModelRouteSendPermit {
        attempt_id: row.try_get("id").unwrap(),
        workspace_id: created.workspace,
        preparation_request_key: prepared.request_key.clone(),
        request_sha256: "0".repeat(64),
        policy_id: Uuid::from_u128(201),
        policy_version: 1,
        policy_digest: "a".repeat(64),
    };
    tx.rollback().await.unwrap();
    assert!(matches!(
        store
            .seal_committed_model_route_response(created.tenant, &wrong_permit, &raw)
            .await,
        Err(Error::InputConflict)
    ));
    assert!(matches!(
        service
            .get_model_route(&context, &prepared.request_key)
            .await,
        Err(Error::SessionRevoked)
    ));
    assert!(matches!(
        service
            .run_model_route(&context, &prepared.request_key)
            .await,
        Err(Error::SessionRevoked)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
