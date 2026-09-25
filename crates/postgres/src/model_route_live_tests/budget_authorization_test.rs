use super::*;
use crate::{BudgetOwnerKeys, PgStore, admin};
use ring::signature::{Ed25519KeyPair, KeyPair};
use tect_application::{Store, TransactionMode};
use tect_domain::{AdvisoryBudgetCeilings, AdvisoryBudgetPolicy};

#[tokio::test]
#[ignore = "requires identity-pinned disposable PG18 and TECT_TEST_* URLs"]
async fn signed_policy_is_shared_by_all_three_runtime_authorization_seams() {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_pool = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    let runtime_pool = PgPool::connect(&std::env::var("TECT_TEST_RUNTIME_URL").unwrap())
        .await
        .unwrap();
    let identity: (String, i64, String) = sqlx::query_as(
        "SELECT current_database(),(SELECT oid::bigint FROM pg_catalog.pg_database WHERE datname=current_database()),(SELECT system_identifier::text FROM pg_catalog.pg_control_system())",
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
    admin::migrate(
        &admin_pool,
        &std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap(),
    )
    .await
    .unwrap();

    let created = super::positive::fixture(&admin_pool, &runtime_pool).await;
    let keypair = Ed25519KeyPair::from_seed_unchecked(&[91u8; 32]).unwrap();
    let key_hex: String = keypair
        .public_key()
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let keys = BudgetOwnerKeys::from_json(&format!(
        r#"[{{"workspace_id":"{}","owner_id":"{}","public_key_hex":"{key_hex}"}}]"#,
        created.workspace, created.owner.principal_id,
    ))
    .unwrap();
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let from = now - 60_000;
    let until = now + 600_000;
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
        AdvisoryBudgetPolicy::digest_for(id, 1, from, until, ceilings),
        from,
        until,
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
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let policy = AdvisoryBudgetPolicy::new(
        id,
        1,
        unsigned.digest().to_owned(),
        from,
        until,
        ceilings,
        created.owner.principal_id,
        signature,
    )
    .unwrap();

    let base_store = PgStore::from_pool(runtime_pool.clone());
    let mut writer = base_store.begin(TransactionMode::ReadWrite).await.unwrap();
    writer.authenticate(&created.owner.auth).await.unwrap();
    writer.set_tenant(created.tenant).await.unwrap();
    writer
        .advisory_budget_policy_store()
        .unwrap()
        .install_budget_policy(created.workspace, &policy)
        .await
        .unwrap();
    writer.commit().await.unwrap();

    for (store, expected) in [
        (base_store, false),
        (
            PgStore::from_pool(runtime_pool).with_budget_owner_keys(keys),
            true,
        ),
    ] {
        let mut tx = store.begin(TransactionMode::ReadOnly).await.unwrap();
        tx.authenticate(&created.owner.auth).await.unwrap();
        tx.set_tenant(created.tenant).await.unwrap();
        assert_eq!(
            tx.advisory_budget_policy_store()
                .unwrap()
                .authorized_budget_policy(created.workspace, now)
                .await
                .unwrap()
                .is_some(),
            expected
        );
        assert_eq!(
            tx.anti_bloat_store()
                .unwrap()
                .authorized_budget_policy(created.workspace, now)
                .await
                .unwrap()
                .is_some(),
            expected
        );
        assert_eq!(
            tx.model_route_attempt_store()
                .unwrap()
                .authorized_budget_policy(created.workspace, now)
                .await
                .unwrap()
                .is_some(),
            expected
        );
        tx.commit().await.unwrap();
    }
}
