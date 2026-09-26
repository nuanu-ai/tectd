use super::*;
use ring::signature::{Ed25519KeyPair, KeyPair};

struct InvalidPreparation;
struct DeclaredProfile(&'static str);
#[async_trait]
impl ModelRouteRankingProvider for DeclaredProfile {
    fn required_profile(&self) -> Option<&str> {
        Some(self.0)
    }
    fn prepare(
        &self,
        _: &tect_application::PreparedModelRouteRecommendation,
    ) -> tect_domain::Result<ModelRoutePreparedAttempt> {
        panic!("profile mismatch must precede prepare")
    }
    async fn attempt_prepared(
        &self,
        _: ModelRoutePreparedAttempt,
        _: ModelRouteSendPermit,
    ) -> tect_domain::Result<Vec<u8>> {
        panic!("profile mismatch must not send")
    }
}
#[async_trait]
impl ModelRouteRankingProvider for InvalidPreparation {
    fn prepare(
        &self,
        _: &tect_application::PreparedModelRouteRecommendation,
    ) -> tect_domain::Result<ModelRoutePreparedAttempt> {
        Err(Error::InvalidArguments)
    }
    async fn attempt_prepared(
        &self,
        _: ModelRoutePreparedAttempt,
        _: ModelRouteSendPermit,
    ) -> tect_domain::Result<Vec<u8>> {
        panic!("invalid configuration must not send")
    }
}

#[tokio::test]
#[ignore = "requires identity-pinned disposable PG96 and TECT_TEST_* URLs"]
async fn signed_positive_budget_unconfigured_and_bad_preflight_make_no_reservation() {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    let runtime = PgPool::connect(&std::env::var("TECT_TEST_RUNTIME_URL").unwrap())
        .await
        .unwrap();
    let identity: (String,i64,String) = sqlx::query_as("SELECT current_database(),(SELECT oid::bigint FROM pg_catalog.pg_database WHERE datname=current_database()),(SELECT system_identifier::text FROM pg_catalog.pg_control_system())").fetch_one(&admin).await.unwrap();
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
    let created = fixture(&admin, &runtime).await;
    let keypair = Ed25519KeyPair::from_seed_unchecked(&[97u8; 32]).unwrap();
    let public_key_hex: String = keypair
        .public_key()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let keys = crate::BudgetOwnerKeys::from_json(&serde_json::json!([{ "workspace_id": created.workspace, "owner_id": created.owner.principal_id, "public_key_hex": public_key_hex }]).to_string()).unwrap();
    let store = PgStore::from_pool(runtime.clone()).with_budget_owner_keys(keys);
    native_seal::choose_profile(&store, &created, "profile-a").await;
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let id = Uuid::new_v4();
    let ceilings = AdvisoryBudgetCeilings {
        provider_calls: 4,
        input_tokens: 100,
        output_tokens: 100,
        request_utf8_bytes: 100_000,
        elapsed_monotonic_ms: 30_000,
        retry_dispatches: 1,
    };
    let unsigned = AdvisoryBudgetPolicy::new(
        id,
        1,
        AdvisoryBudgetPolicy::digest_for(id, 1, now - 60_000, now + 600_000, ceilings),
        now - 60_000,
        now + 600_000,
        ceilings,
        created.owner.principal_id,
        "0".repeat(128),
    )
    .unwrap();
    let signature: String = keypair
        .sign(
            &unsigned
                .approval_signing_message(created.workspace)
                .unwrap(),
        )
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let policy = AdvisoryBudgetPolicy::new(
        id,
        1,
        unsigned.digest().into(),
        now - 60_000,
        now + 600_000,
        ceilings,
        created.owner.principal_id,
        signature,
    )
    .unwrap();
    let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    tx.authenticate(&created.owner.auth).await.unwrap();
    tx.set_tenant(created.tenant).await.unwrap();
    tx.advisory_budget_policy_store()
        .unwrap()
        .install_budget_policy(created.workspace, &policy)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    for (index, provider) in [
        &tect_application::DisabledModelRouteRankingProvider as &dyn ModelRouteRankingProvider,
        &InvalidPreparation,
        &DeclaredProfile("profile-b"),
    ]
    .into_iter()
    .enumerate()
    {
        let prepared =
            negative_cases::prepare_case(&store, &runtime, &created, &format!("preflight-{index}"))
                .await;
        assert_eq!(prepared.preparation, ModelRoutePreparation::Prepared);
        let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
        tx.authenticate(&created.owner.auth).await.unwrap();
        tx.set_tenant(created.tenant).await.unwrap();
        let attempts = tx.model_route_attempt_store().unwrap();
        assert_eq!(
            attempts
                .authorized_budget_policy(created.workspace, now)
                .await
                .unwrap(),
            Some(policy.clone())
        );
        let start = prepare_model_route_send(
            attempts,
            provider,
            &prepared,
            ModelRouteInvocation {
                session_id: created.invocation_session,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            start,
            ModelRouteSendStart::NoCall(tect_application::ModelRouteRunNoCall::ProviderUnavailable)
        );
        tx.commit().await.unwrap();
    }
    let counts: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM model_route_advisory_attempts WHERE workspace_id=$1 AND state='no_call'),(SELECT count(*) FROM model_route_budget_reservations WHERE workspace_id=$1),(SELECT count(*) FROM model_route_budget_consumptions WHERE workspace_id=$1)").bind(created.workspace).fetch_one(&admin).await.unwrap();
    assert_eq!(counts, (3, 0, 0));
}
