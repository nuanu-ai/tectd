//! Application-to-PostgreSQL Matrix readiness proof on synthetic fixtures.
//! Owner-reported material has no independent verification path yet, so even
//! an explicitly configured provider and positive budget must remain no-call.

use async_trait::async_trait;
use sqlx::{PgPool, postgres::PgConnectOptions};
use std::{
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tect_application::{
    MatrixAdviceProvider, MatrixBudgetAuthorization, MatrixBudgetPolicy, MatrixBudgetRequest,
    MatrixProviderIdentity, MatrixProviderRequest, MatrixProviderResponse,
    MatrixStartedDispatchPermit, PreparedMatrixAdviceAttempt, RecordMatrixTask,
    RequestEngineeringAdvisory, WorkspaceService,
};
use tect_domain::{
    AdvisoryModelConfiguration, AdvisoryOpportunityState, AdvisoryProviderProfileRef,
    AdvisoryReason, AdvisoryRequestPreference, ConfigureWorkspaceAdvisory, EngineeringCandidate,
    EngineeringChoiceSet, EngineeringMatrixInput, MATRIX_CHOICE_SET_SCHEMA, RequestContext,
    WorkspaceAdvisoryMode,
};
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

const PROFILE: &str = "fixture-matrix-provider";
const MODEL: &str = "fixture-matrix-model";

struct PositiveFixtureBudget(Arc<AtomicUsize>);

#[async_trait]
impl MatrixBudgetPolicy for PositiveFixtureBudget {
    async fn authorize(
        &self,
        _: &MatrixBudgetRequest,
    ) -> tect_domain::Result<Option<MatrixBudgetAuthorization>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Some(MatrixBudgetAuthorization {
            policy_id: "synthetic-authorized-budget".into(),
        }))
    }
}

struct CountingFixtureProvider(Arc<AtomicUsize>);

#[async_trait]
impl MatrixAdviceProvider for CountingFixtureProvider {
    fn identity(&self) -> Option<MatrixProviderIdentity> {
        Some(MatrixProviderIdentity {
            provider_profile_ref: AdvisoryProviderProfileRef { id: PROFILE.into() },
            model_configuration: AdvisoryModelConfiguration {
                model: MODEL.into(),
            },
            destination: "https://fixture.invalid/matrix".into(),
            wire_version: "fixture-matrix/1".into(),
        })
    }

    fn prepare(
        &self,
        _: &MatrixProviderRequest,
    ) -> tect_domain::Result<PreparedMatrixAdviceAttempt> {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("unverified Matrix must not prepare a provider request")
    }

    async fn attempt_prepared(
        &self,
        _: PreparedMatrixAdviceAttempt,
        _: MatrixStartedDispatchPermit,
    ) -> tect_domain::Result<MatrixProviderResponse> {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("unverified Matrix must not attempt provider transport")
    }
}

/// Validate both URLs and their shared backend before migration or fixture writes.
async fn disposable_pair() -> (PgPool, String) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    assert_eq!(role, "tect_ci");
    let admin_options = PgConnectOptions::from_str(&admin_url).unwrap();
    let runtime_options = PgConnectOptions::from_str(&runtime_url).unwrap();
    assert_eq!(admin_options.get_username(), "postgres");
    assert_eq!(runtime_options.get_username(), role);
    for options in [&admin_options, &runtime_options] {
        assert_eq!(options.get_database(), Some("tect_test"));
        let socket = options
            .get_socket()
            .expect("disposable Unix socket required");
        assert!(socket.is_absolute());
        assert!(socket.to_string_lossy().contains("tectd-matrix-pg18.6-"));
    }
    assert_eq!(admin_options.get_socket(), runtime_options.get_socket());
    assert_eq!(admin_options.get_port(), runtime_options.get_port());
    let admin_pool = PgPool::connect_with(admin_options).await.unwrap();
    let runtime_pool = PgPool::connect_with(runtime_options).await.unwrap();
    let (version, database, user, oid, system): (i32, String, String, i64, String) =
        sqlx::query_as(
            "SELECT current_setting('server_version_num')::integer,current_database(),current_user, \
             (SELECT oid::bigint FROM pg_database WHERE datname=current_database()), \
             (SELECT system_identifier::text FROM pg_control_system())",
        )
        .fetch_one(&admin_pool)
        .await
        .unwrap();
    assert_eq!(
        (version, database.as_str(), user.as_str()),
        (180006, "tect_test", "postgres")
    );
    assert!(system.parse::<u64>().is_ok_and(|id| id != 0));
    let mut runtime = runtime_pool.acquire().await.unwrap();
    let (runtime_oid, runtime_user, pid): (i64, String, i32) = sqlx::query_as(
        "SELECT (SELECT oid::bigint FROM pg_database WHERE datname=current_database()),current_user,pg_backend_pid()",
    )
    .fetch_one(&mut *runtime)
    .await
    .unwrap();
    assert_eq!((runtime_oid, runtime_user.as_str()), (oid, role.as_str()));
    let observed: (String, String) =
        sqlx::query_as("SELECT datname,usename FROM pg_stat_activity WHERE pid=$1")
            .bind(pid)
            .fetch_one(&admin_pool)
            .await
            .unwrap();
    assert_eq!(observed, ("tect_test".into(), role));
    (admin_pool, runtime_url)
}

fn owner_input(complete: bool) -> EngineeringMatrixInput {
    let value = if complete {
        serde_json::json!({
            "mode": {"state":"known","value":"demo","provenance":"owner report"},
            "envelope": {
                "scale":{"state":"known","value":"one synthetic request","provenance":"owner report"},
                "operational_facts":{"state":"reported","entries":[{
                    "name":"environment","fact":{"state":"known","value":"synthetic","provenance":"owner report"}
                }]}
            },
            "criticality":{"state":"known","value":"no protected guarantee","provenance":"owner report"},
            "intent":{"state":"known","value":{"kind":"other","description":"demo"},"provenance":"owner report"},
            "urgency":{"state":"known","value":"ordinary","provenance":"owner report"},
            "promised_behavior":{"state":"known","value":"real demo","provenance":"owner report"},
            "promised_proof":{"state":"known","value":"demo check","provenance":"owner report"},
            "affected_guarantees":{"state":"known_empty","provenance":"owner report"},
            "actual_exposure":{"state":"known","value":false,"provenance":"owner report"},
            "demand_commitment":{"state":"known","value":"no_commitment","provenance":"owner report"},
            "latency_commitment":{"state":"known","value":"no_commitment","provenance":"owner report"},
            "urgent_repair":{"state":"known","value":false,"provenance":"owner report"}
        })
    } else {
        serde_json::json!({
            "mode": {"state":"absent"},
            "envelope": {"scale":{"state":"absent"},"operational_facts":{"state":"absent"}},
            "criticality":{"state":"absent"}, "intent":{"state":"absent"},
            "urgency":{"state":"absent"}, "promised_behavior":{"state":"absent"},
            "promised_proof":{"state":"absent"}, "affected_guarantees":{"state":"absent"},
            "actual_exposure":{"state":"absent"}, "demand_commitment":{"state":"absent"},
            "latency_commitment":{"state":"absent"}, "urgent_repair":{"state":"absent"}
        })
    };
    serde_json::from_value(value).unwrap()
}

async fn no_call_case(pool: &PgPool, runtime_url: &str, complete: bool) {
    let owner = admin::enroll_host(pool, None, vec![]).await.unwrap();
    let context = RequestContext {
        auth: owner.auth,
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: format!("matrix-no-call-{}", Uuid::new_v4().simple()),
    };
    let provider_entries = Arc::new(AtomicUsize::new(0));
    let budget_entries = Arc::new(AtomicUsize::new(0));
    let service = WorkspaceService::new(
        Arc::new(PgStore::connect(runtime_url, 4).await.unwrap()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    )
    .with_matrix_advisory_adapters(
        Arc::new(CountingFixtureProvider(provider_entries.clone())),
        Arc::new(PositiveFixtureBudget(budget_entries.clone())),
    );
    let workspace = service
        .open_workspace(&context)
        .await
        .unwrap()
        .workspace
        .unwrap();
    let task_id = Uuid::new_v4();
    let input = owner_input(complete);
    let choice_set = EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: format!("choice-{}", Uuid::new_v4().simple()),
        version: 1,
        task_id: task_id.to_string(),
        task_revision: "1".into(),
        decision_question: "Which owner-authored approach?".into(),
        candidates: ["a", "b"]
            .into_iter()
            .map(|candidate_id| EngineeringCandidate {
                candidate_id: candidate_id.into(),
                title: format!("Approach {candidate_id}"),
                approach: format!("Owner-authored approach {candidate_id}"),
                assumption_fact_ids: vec![],
            })
            .collect(),
    };
    service
        .record_matrix_task(
            &context,
            &RecordMatrixTask {
                task_id,
                revision: 1,
                expected_current_revision: 0,
                request_id: Uuid::new_v4(),
                input,
                choice_set: Some(choice_set),
            },
        )
        .await
        .unwrap();
    let config = service
        .configure_advisory(
            &context,
            &ConfigureWorkspaceAdvisory {
                expected_revision: 0,
                mode: WorkspaceAdvisoryMode::Optional,
                provider_profile_ref: Some(AdvisoryProviderProfileRef { id: PROFILE.into() }),
                model_configuration: Some(AdvisoryModelConfiguration {
                    model: MODEL.into(),
                }),
            },
        )
        .await
        .unwrap();
    assert_eq!(config.revision, 1);
    let request = RequestEngineeringAdvisory {
        task_id,
        expected_task_revision: 1,
        request_key: format!("matrix-readiness-{}", Uuid::new_v4()),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
    };
    let receipt = service
        .request_engineering_advisory(&context, &request)
        .await
        .unwrap();
    let expected = if complete {
        AdvisoryReason::MatrixSourceUnverified
    } else {
        AdvisoryReason::MatrixEvidenceUnresolved
    };
    assert_eq!(receipt.state, AdvisoryOpportunityState::NoCall);
    assert_eq!(receipt.primary_reason, expected);
    let replay = service
        .request_engineering_advisory(&context, &request)
        .await
        .unwrap();
    assert_eq!(replay.id, receipt.id);
    assert_eq!(replay.primary_reason, expected);
    let saved = service
        .get_engineering_advisory(&context, task_id, &request.request_key)
        .await
        .unwrap();
    assert_eq!(saved.id, receipt.id);
    assert_eq!(saved.primary_reason, expected);
    assert_eq!(provider_entries.load(Ordering::SeqCst), 0);
    assert_eq!(budget_entries.load(Ordering::SeqCst), 0);
    let persisted: (String, String, Uuid, Uuid) = sqlx::query_as(
        "SELECT state,primary_reason,tenant_id,workspace_id FROM advisory_opportunity WHERE id=$1",
    )
    .bind(receipt.id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        persisted,
        (
            "no_call".into(),
            expected.as_str().into(),
            owner.tenant_id,
            workspace.id
        )
    );
    let dispatch_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE opportunity_id=$1")
            .bind(receipt.id)
            .fetch_one(pool)
            .await
            .unwrap();
    let advice_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_matrix_advice WHERE opportunity_id=$1")
            .bind(receipt.id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!((dispatch_count, advice_count), (0, 0));
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 18.6 opt-in and TECT_TEST_* required"]
async fn owner_reported_matrix_remains_no_call_even_with_provider_and_positive_budget() {
    let (pool, runtime_url) = disposable_pair().await;
    admin::migrate(&pool, "tect_ci").await.unwrap();
    no_call_case(&pool, &runtime_url, false).await;
    no_call_case(&pool, &runtime_url, true).await;
}
