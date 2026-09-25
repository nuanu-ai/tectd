const MIGRATION: &str = include_str!("../migrations/0055_matrix_verification.sql");
const GRANTS: &str = include_str!("admin/migration.rs");
const STORE: &str = include_str!("matrix_verification_store.rs");
const ADVICE_LINK: &str =
    include_str!("../migrations/0056_matrix_advisory_verification_binding.sql");
const ADVICE_DISPATCH_AUTHORIZATION: &str = include_str!("advisory/dispatch/authorization.rs");
const ADVICE_DISPATCH_LIFECYCLE: &str = include_str!("advisory/dispatch/lifecycle.rs");
const ADVICE_STORE: &str = include_str!("matrix_advice_store/implementation.rs");
const STALE_REASON: &str = include_str!("../migrations/0057_matrix_verification_stale_reason.sql");

#[test]
fn post_response_verification_drift_is_terminal_and_matrix_only() {
    assert!(STALE_REASON.contains("'matrix_verification_stale'"));
    assert!(
        STALE_REASON
            .contains("AND capability = 'engineering_profile' AND work_item_kind = 'matrix_task'")
    );
    assert!(STALE_REASON.contains("NOT VALID"));
    assert!(ADVICE_DISPATCH_LIFECYCLE.contains("Some(AdvisoryReason::MatrixVerificationStale)"));
    assert!(
        ADVICE_STORE.contains(
            "record.binding.verification_digest.as_deref() != Some(latest_digest.as_str())"
        )
    );
    assert!(ADVICE_STORE.contains("EXTRACT(EPOCH FROM pg_catalog.clock_timestamp())"));
}

#[test]
fn positive_matrix_advice_requires_exact_immutable_verification() {
    for required in [
        "ADD COLUMN matrix_verification_digest text",
        "matrix_verification_digest IS NOT NULL))) NOT VALID",
        "(tenant_id, workspace_id, work_item_id,",
        "matrix_task_revision, matrix_verification_digest)",
        "(tenant_id, workspace_id, task_id, task_revision, record_digest)",
    ] {
        assert!(ADVICE_LINK.contains(required), "missing {required}");
    }
    for required in [
        "ORDER BY verified_at DESC,id DESC LIMIT 1",
        "verification_digest != expected_verification",
        "verified_input_digest != input_digest",
        "expires_at <=",
    ] {
        assert!(
            ADVICE_DISPATCH_AUTHORIZATION.contains(required),
            "missing {required}"
        );
    }
    assert!(ADVICE_STORE.contains("verification_digest != record.binding.verification_digest"));
    assert!(ADVICE_STORE.contains("o.matrix_verification_digest"));
}

#[test]
fn verification_and_bindings_are_exactly_revision_bound_and_immutable() {
    for required in [
        "UNIQUE (tenant_id, workspace_id, task_id, revision, input_digest)",
        "CREATE TABLE matrix_verifications (",
        "CREATE TABLE matrix_verification_bindings (",
        "(tenant_id, workspace_id, task_id, task_revision, input_digest)",
        "(tenant_id, workspace_id, task_id, revision, input_digest)",
        "verification_reason = 'matrix_facts_verified'",
        "policy_version text NOT NULL",
        "record_digest text NOT NULL",
        "PRIMARY KEY (tenant_id, workspace_id, verification_id, fact_path)",
        "BEFORE UPDATE OR DELETE ON matrix_verifications",
        "BEFORE UPDATE OR DELETE ON matrix_verification_bindings",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
}

#[test]
fn verifier_identity_is_database_enforced_without_task_update_authority() {
    for required in [
        "NEW.verifier_session_id",
        "r.recorded_by_principal_id=NEW.owner_principal_id",
        "p.id=NEW.verifier_principal_id AND p.role='verifier'",
        "p.id<>NEW.owner_principal_id",
        "AND NOT s.revoked AND NOT h.revoked",
        "FOR SHARE OF t, r, s, h, p, m",
        "ENABLE ROW LEVEL SECURITY",
        "FORCE ROW LEVEL SECURITY",
        "REVOKE ALL PRIVILEGES ON TABLE matrix_verifications, matrix_verification_bindings FROM PUBLIC",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    assert!(GRANTS.contains("GRANT SELECT, INSERT ON TABLE matrix_verifications, matrix_verification_bindings TO {quoted_role}"));
    assert!(!GRANTS.contains("GRANT UPDATE ON TABLE matrix_verifications"));
    assert!(!GRANTS.contains("GRANT DELETE ON TABLE matrix_verifications"));
    assert!(!GRANTS.contains("GRANT UPDATE(current_revision) ON TABLE matrix_task_revisions"));
}

#[test]
fn store_rechecks_current_head_and_reconstructs_exact_record() {
    for required in [
        "t.current_revision=$4 FOR UPDATE OF t",
        "evaluate_matrix_verification(",
        "if prior == *record",
        "ORDER BY v.verified_at DESC,v.id DESC LIMIT 1",
        "canonical_digest()",
    ] {
        assert!(STORE.contains(required), "missing {required}");
    }
}

#[tokio::test]
#[ignore = "requires an explicitly disposable PostgreSQL 18 test database"]
async fn live_verification_insert_requires_active_independent_verifier_and_is_immutable() {
    use crate::admin;
    use sqlx::{PgPool, Postgres, Transaction};
    use uuid::Uuid;

    async fn set_tenant(tx: &mut Transaction<'_, Postgres>, tenant_id: Uuid) {
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
            .bind(tenant_id.to_string())
            .execute(&mut **tx)
            .await
            .unwrap();
    }

    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_pool = PgPool::connect(&admin_url).await.unwrap();
    let version: i32 = sqlx::query_scalar("SELECT current_setting('server_version_num')::integer")
        .fetch_one(&admin_pool)
        .await
        .unwrap();
    assert!((180000..190000).contains(&version));
    admin::migrate(&admin_pool, &role).await.unwrap();
    let runtime_pool = PgPool::connect(&runtime_url).await.unwrap();
    let owner = admin::enroll_host(&admin_pool, None, vec![]).await.unwrap();
    let tenant_id = owner.tenant_id;
    let workspace_id = Uuid::new_v4();
    let owner_session = Uuid::new_v4();
    let verifier_id = Uuid::new_v4();
    let verifier_host = Uuid::new_v4();
    let verifier_session = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    sqlx::query("INSERT INTO workspaces (tenant_id,id,key) VALUES ($1,$2,$3)")
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(format!("verify-{}", workspace_id.simple()))
        .execute(&admin_pool)
        .await
        .unwrap();
    for principal in [owner.principal_id, verifier_id] {
        if principal == verifier_id {
            sqlx::query("INSERT INTO principals (id,tenant_id,role) VALUES ($1,$2,'verifier')")
                .bind(verifier_id)
                .bind(tenant_id)
                .execute(&admin_pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO hosts (id,tenant_id,principal_id,credential_digest) VALUES ($1,$2,$3,$4)")
                .bind(verifier_host)
                .bind(tenant_id)
                .bind(verifier_id)
                .bind(format!("{:064x}", verifier_host.as_u128()))
                .execute(&admin_pool)
                .await
                .unwrap();
        }
        sqlx::query(
            "INSERT INTO memberships (tenant_id,workspace_id,principal_id) VALUES ($1,$2,$3)",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(principal)
        .execute(&admin_pool)
        .await
        .unwrap();
    }
    for (session, host) in [
        (owner_session, owner.auth.host_id),
        (verifier_session, verifier_host),
    ] {
        sqlx::query("INSERT INTO agent_sessions (id,tenant_id,host_id,workspace_id,native_session_id) VALUES ($1,$2,$3,$4,$5)")
            .bind(session)
            .bind(tenant_id)
            .bind(host)
            .bind(workspace_id)
            .bind(Uuid::new_v4().to_string())
            .execute(&admin_pool)
            .await
            .unwrap();
    }
    let mut tx = runtime_pool.begin().await.unwrap();
    set_tenant(&mut tx, tenant_id).await;
    sqlx::query(
        "INSERT INTO matrix_tasks (tenant_id,workspace_id,id,current_revision) VALUES ($1,$2,$3,1)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO matrix_task_revisions (tenant_id,workspace_id,task_id,revision,request_id,input_schema,canonical_input,input_digest,recorded_by_principal_id,recorded_by_session_id) VALUES ($1,$2,$3,1,$4,'tect.engineering-matrix-input/1',$5,$6,$7,$8)")
        .bind(tenant_id).bind(workspace_id).bind(task_id).bind(Uuid::new_v4())
        .bind(serde_json::json!({"mode":"build","envelope":{},"criticality":"normal","intent":"x","urgency":"normal","promised_behavior":[],"promised_proof":[],"affected_guarantees":[],"actual_exposure":[],"demand_commitment":null,"latency_commitment":null,"urgent_repair":false}))
        .bind("a".repeat(64)).bind(owner.principal_id).bind(owner_session)
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();

    let mut tx = runtime_pool.begin().await.unwrap();
    set_tenant(&mut tx, tenant_id).await;
    let verification_id: Uuid = sqlx::query_scalar("INSERT INTO matrix_verifications (tenant_id,workspace_id,task_id,task_revision,input_digest,schema,owner_principal_id,verifier_principal_id,verifier_session_id,verification_reason,policy_version,record_digest) VALUES ($1,$2,$3,1,$4,'tect.matrix-verification/1',$5,$6,$7,'matrix_facts_verified','test-policy/1',$8) RETURNING id")
        .bind(tenant_id).bind(workspace_id).bind(task_id).bind("a".repeat(64))
        .bind(owner.principal_id).bind(verifier_id).bind(verifier_session).bind("b".repeat(64))
        .fetch_one(&mut *tx).await.unwrap();
    let denied = sqlx::query("UPDATE matrix_verifications SET policy_version='changed' WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant_id).bind(workspace_id).bind(verification_id)
        .execute(&mut *tx).await.unwrap_err();
    assert_eq!(
        denied.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    tx.rollback().await.unwrap();

    let mut tx = runtime_pool.begin().await.unwrap();
    set_tenant(&mut tx, tenant_id).await;
    let denied = sqlx::query("INSERT INTO matrix_verifications (tenant_id,workspace_id,task_id,task_revision,input_digest,schema,owner_principal_id,verifier_principal_id,verifier_session_id,verification_reason,policy_version,record_digest) VALUES ($1,$2,$3,1,$4,'tect.matrix-verification/1',$5,$6,$7,'matrix_facts_verified','test-policy/1',$8)")
        .bind(tenant_id).bind(workspace_id).bind(task_id).bind("a".repeat(64))
        .bind(owner.principal_id).bind(verifier_id).bind(owner_session).bind("c".repeat(64))
        .execute(&mut *tx).await.unwrap_err();
    assert_eq!(
        denied.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    tx.rollback().await.unwrap();
}
