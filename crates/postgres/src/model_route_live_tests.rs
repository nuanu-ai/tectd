//! Negative, rollback-only live proof. A positive route receipt requires a
//! separately authored current Matrix-selected native save fixture.
use crate::store::PgUnitOfWork;
use sqlx::{PgPool, Row};
use tect_application::{ModelRouteRecommendationStore, PreparedModelRouteRecommendation};
use tect_domain::{
    AdvisoryRequestPreference, Error, MatrixPlanningSelection, ModelRouteFact, ModelRouteRecord,
    ModelRouteSelectionLink, ModelRouteWorkContext,
};
use uuid::Uuid;

fn sqlstate(error: &sqlx::Error) -> Option<String> {
    error
        .as_database_error()
        .and_then(|error| error.code())
        .map(|code| code.to_string())
}

async fn refused_insert(
    pool: &PgPool,
    session_tenant: Uuid,
    row_tenant: Uuid,
    observed_actual: serde_json::Value,
) -> String {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(session_tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let payload = serde_json::json!({
        "routes": {"recommended_route_id": null, "observed_actual": observed_actual}
    });
    let error = sqlx::query(
        "INSERT INTO model_route_preparations \
         (tenant_id,workspace_id,request_key,disposition_id,candidate_set_id,caller_request_id, \
          work_node_id,work_node_revision,task_id,task_revision,advisory_mode, \
          advisory_config_revision,work_digest,prepared_payload) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,1,$8,1,'disabled',0,$9,$10)",
    )
    .bind(row_tenant)
    .bind(Uuid::new_v4())
    .bind(format!("model-route-{}", Uuid::new_v4()))
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind("0".repeat(64))
    .bind(payload)
    .execute(&mut *tx)
    .await
    .unwrap_err();
    tx.rollback().await.unwrap();
    sqlstate(&error).expect("database SQLSTATE")
}

fn missing_link_preparation(workspace_id: Uuid) -> PreparedModelRouteRecommendation {
    let candidate_set_id = Uuid::new_v4();
    let caller_request_id = Uuid::new_v4();
    let node_id = Uuid::new_v4();
    PreparedModelRouteRecommendation {
        workspace_id,
        request_key: format!("missing-link-{}", Uuid::new_v4()),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        advisory_config_revision: 0,
        work: ModelRouteWorkContext {
            approved_matrix_selection: MatrixPlanningSelection {
                task_id: Uuid::new_v4(),
                task_revision: 1,
                disposition_id: Uuid::new_v4(),
                selected_choice_id: "choice-a".into(),
                expected_input_digest: "a".repeat(64),
                expected_choice_set_digest: "b".repeat(64),
                expected_verification_digest: "c".repeat(64),
                mapped_draft_node_indices: vec![0],
            },
            selection_link: ModelRouteSelectionLink {
                candidate_set_id,
                caller_request_id,
                mapped_draft_node_index: 0,
                mapped_work_node_id: node_id,
                mapped_work_node_revision: 1,
            },
            role: ModelRouteFact::Unknown,
            tool: ModelRouteFact::Unknown,
            data_class: ModelRouteFact::Unknown,
            host_capabilities: ModelRouteFact::Unknown,
            remaining_budget_units: ModelRouteFact::Unknown,
            available_latency_ms: ModelRouteFact::Unknown,
        },
        catalogue: None,
        eligible: None,
        preparation: tect_application::ModelRoutePreparation::WorkspaceDisabled,
        routes: ModelRouteRecord {
            requested_route_id: None,
            recommended_route_id: None,
            observed_actual: None,
        },
    }
}

#[tokio::test]
#[ignore = "requires identity-pinned disposable PG18 fixture and TECT_TEST_* URLs"]
async fn model_route_missing_link_rls_and_no_execution_are_denied_live() {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let admin_pool = PgPool::connect(&admin_url).await.unwrap();
    let runtime_pool = PgPool::connect(&runtime_url).await.unwrap();
    let identity: (String, i64, String) = sqlx::query_as(
        "SELECT current_database(), \
         (SELECT oid::bigint FROM pg_catalog.pg_database WHERE datname=current_database()), \
         (SELECT system_identifier::text FROM pg_catalog.pg_control_system())",
    )
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert_eq!(identity.0, "tect_test");
    assert_eq!(
        identity.1.to_string(),
        std::env::var("TECT_TEST_EXPECTED_DB_OID").unwrap()
    );
    assert_eq!(
        identity.2,
        std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").unwrap()
    );
    let ledger = sqlx::query(
        "SELECT count(*) AS n,max(version) AS top,bool_and(success) AS valid FROM _sqlx_migrations",
    )
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert!(ledger.try_get::<i64, _>("n").unwrap() >= 80);
    assert!(ledger.try_get::<i64, _>("top").unwrap() >= 80);
    assert!(ledger.try_get::<bool, _>("valid").unwrap());
    let role: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&runtime_pool)
        .await
        .unwrap();
    assert_eq!(role, "tect_ci");

    let tenant = Uuid::new_v4();
    let other_tenant = Uuid::new_v4();
    assert_eq!(
        refused_insert(&runtime_pool, tenant, other_tenant, serde_json::Value::Null).await,
        "42501"
    );
    assert_eq!(
        refused_insert(
            &runtime_pool,
            tenant,
            tenant,
            serde_json::json!({"route_id":"forged"})
        )
        .await,
        "23514"
    );
    assert_eq!(
        refused_insert(&runtime_pool, tenant, tenant, serde_json::Value::Null).await,
        "23503"
    );

    let workspace = Uuid::new_v4();
    let prepared = missing_link_preparation(workspace);
    let mut unit = PgUnitOfWork::test_begin(&runtime_pool, tenant).await;
    assert_eq!(unit.capture(&prepared).await, Err(Error::StaleContext));
    assert_eq!(
        unit.by_request(workspace, &prepared.request_key)
            .await
            .unwrap(),
        None
    );
    drop(unit); // rollback; this test leaves no rows.
}
