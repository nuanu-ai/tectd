//! Application-to-PostgreSQL Matrix dispatch proof; all provider bytes are synthetic.
//! Run only with TECT_TEST_DISPOSABLE_PG=1 and the isolated PG18 test URLs.

use async_trait::async_trait;
use sha2::{Digest, Sha256};
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
    EngineeringChoiceSet, EngineeringMatrixInput, MATRIX_CHOICE_SET_SCHEMA, MatrixRanking,
    RequestContext, WorkspaceAdvisoryMode,
};
use tect_postgres::{PgStore, admin};
use tokio::sync::Notify;
use uuid::Uuid;

const PROFILE: &str = "fixture-matrix-provider";
const MODEL: &str = "fixture-matrix-model";

struct AllowFixtureBudget;

#[async_trait]
impl MatrixBudgetPolicy for AllowFixtureBudget {
    async fn authorize(
        &self,
        request: &MatrixBudgetRequest,
    ) -> tect_domain::Result<Option<MatrixBudgetAuthorization>> {
        assert_eq!(request.provider_profile_ref.id, PROFILE);
        assert_eq!(request.model_configuration.model, MODEL);
        assert_eq!(request.destination, "https://fixture.invalid/matrix");
        assert_eq!(request.wire_version, "fixture-matrix/1");
        Ok(Some(MatrixBudgetAuthorization {
            policy_id: "synthetic-authorized-budget".into(),
        }))
    }
}

struct FixtureProvider {
    calls: Arc<AtomicUsize>,
    abstain: bool,
    admin_pool: PgPool,
    gate: Option<(Arc<Notify>, Arc<Notify>)>,
}

#[async_trait]
impl MatrixAdviceProvider for FixtureProvider {
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
        request: &MatrixProviderRequest,
    ) -> tect_domain::Result<PreparedMatrixAdviceAttempt> {
        PreparedMatrixAdviceAttempt::new(
            request,
            self.identity().unwrap(),
            serde_json::to_vec(&serde_json::json!({
                "binding": {
                    "task_id": request.binding().task_id,
                    "task_revision": request.binding().task_revision,
                    "evaluation_digest": request.binding().evaluation_digest,
                },
                "choice_set": request.revision().choice_set,
                "composition": request.composition(),
            }))
            .unwrap(),
        )
    }

    async fn attempt_prepared(
        &self,
        prepared: PreparedMatrixAdviceAttempt,
        permit: MatrixStartedDispatchPermit,
    ) -> tect_domain::Result<MatrixProviderResponse> {
        let binding = prepared.binding().clone();
        let identity = prepared.identity().clone();
        let (opportunity_id, dispatch_id, configuration_digest, request_payload): (
            Uuid,
            Uuid,
            String,
            Vec<u8>,
        ) = sqlx::query_as(
            "SELECT o.id,d.id,d.configuration_digest,d.request_payload \
                 FROM advisory_dispatch d JOIN advisory_opportunity o ON o.id=d.opportunity_id \
                 WHERE o.work_item_id=$1 AND d.material_digest=$2 AND d.state='sending'",
        )
        .bind(binding.task_id)
        .bind(&binding.evaluation_digest)
        .fetch_one(&self.admin_pool)
        .await
        .unwrap();
        assert_eq!(request_payload, prepared.body());
        assert!(permit.permits(
            opportunity_id,
            dispatch_id,
            &configuration_digest,
            &prepared
        ));
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some((entered, release)) = &self.gate {
            entered.notify_one();
            release.notified().await;
        }
        let raw = if self.abstain {
            b"fixture:abstained".to_vec()
        } else {
            b"fixture:ranked-b-a".to_vec()
        };
        Ok(MatrixProviderResponse {
            binding,
            provider_profile_ref: identity.provider_profile_ref,
            model_configuration: identity.model_configuration,
            response_payload_sha256: format!("{:x}", Sha256::digest(&raw)),
            raw_response_payload: raw,
            ranking: if self.abstain {
                MatrixRanking::Abstained {
                    ranked_candidate_ids: vec![],
                    recommended_candidate_id: None,
                }
            } else {
                MatrixRanking::Ranked {
                    ranked_candidate_ids: vec!["b".into(), "a".into()],
                    recommended_candidate_id: "b".into(),
                }
            },
            input_tokens: Some(7),
            output_tokens: Some(3),
        })
    }
}

async fn disposable_pair() -> (PgPool, String, String) {
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
            .expect("disposable test requires a Unix socket");
        assert!(socket.is_absolute());
        assert!(socket.to_string_lossy().contains("tectd-matrix-pg18.6-"));
    }
    assert_eq!(admin_options.get_socket(), runtime_options.get_socket());
    assert_eq!(admin_options.get_port(), runtime_options.get_port());
    let admin_pool = PgPool::connect_with(admin_options).await.unwrap();
    let runtime_pool = PgPool::connect_with(runtime_options).await.unwrap();
    let (version, database, user, oid, system): (i32, String, String, i64, String) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),current_user, \
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()), \
         (SELECT system_identifier::text FROM pg_control_system())"
    ).fetch_one(&admin_pool).await.unwrap();
    assert_eq!(
        (version, database.as_str(), user.as_str()),
        (180006, "tect_test", "postgres")
    );
    assert!(system.parse::<u64>().is_ok_and(|id| id != 0));
    let mut runtime = runtime_pool.acquire().await.unwrap();
    let (runtime_oid, runtime_user, pid): (i64, String, i32) = sqlx::query_as(
        "SELECT (SELECT oid::bigint FROM pg_database WHERE datname=current_database()),current_user,pg_backend_pid()"
    ).fetch_one(&mut *runtime).await.unwrap();
    assert_eq!((runtime_oid, runtime_user.as_str()), (oid, role.as_str()));
    let observed: (String, String) =
        sqlx::query_as("SELECT datname,usename FROM pg_stat_activity WHERE pid=$1")
            .bind(pid)
            .fetch_one(&admin_pool)
            .await
            .unwrap();
    assert_eq!(observed, ("tect_test".into(), role));
    (admin_pool, admin_url, runtime_url)
}

fn input() -> EngineeringMatrixInput {
    serde_json::from_value(serde_json::json!({
        "mode": {"state":"absent"},
        "envelope": {"scale":{"state":"absent"},"operational_facts":{"state":"absent"}},
        "criticality":{"state":"absent"}, "intent":{"state":"absent"},
        "urgency":{"state":"absent"}, "promised_behavior":{"state":"absent"},
        "promised_proof":{"state":"absent"}, "affected_guarantees":{"state":"absent"},
        "actual_exposure":{"state":"absent"}, "demand_commitment":{"state":"absent"},
        "latency_commitment":{"state":"absent"}, "urgent_repair":{"state":"absent"}
    }))
    .unwrap()
}

async fn case(pool: &PgPool, runtime_url: &str, abstain: bool, advance_during_send: bool) {
    let owner = admin::enroll_host(pool, None, vec![]).await.unwrap();
    let context = RequestContext {
        auth: owner.auth,
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: format!("matrix-active-{}", Uuid::new_v4().simple()),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(PgStore::connect(runtime_url, 4).await.unwrap());
    let default_service = WorkspaceService::new(
        store.clone(),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    );
    let workspace = default_service
        .open_workspace(&context)
        .await
        .unwrap()
        .workspace
        .unwrap();
    let task_id = Uuid::new_v4();
    let choice_set = EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: format!("set-{}", Uuid::new_v4().simple()),
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
    default_service
        .record_matrix_task(
            &context,
            &RecordMatrixTask {
                task_id,
                revision: 1,
                expected_current_revision: 0,
                request_id: Uuid::new_v4(),
                input: input(),
                choice_set: Some(choice_set),
            },
        )
        .await
        .unwrap();
    let request = |suffix: &str| RequestEngineeringAdvisory {
        task_id,
        expected_task_revision: 1,
        request_key: format!("matrix-fixture-{suffix}-{}", Uuid::new_v4()),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
    };
    let disabled = request("disabled");
    let no_call = default_service
        .request_engineering_advisory(&context, &disabled)
        .await
        .unwrap();
    assert_eq!(no_call.state, AdvisoryOpportunityState::NoCall);
    let no_call_attempts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE opportunity_id=$1")
            .bind(no_call.id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(no_call_attempts, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let config = default_service
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
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let provider = Arc::new(FixtureProvider {
        calls: calls.clone(),
        abstain,
        admin_pool: pool.clone(),
        gate: advance_during_send.then(|| (entered.clone(), release.clone())),
    });
    let budget_denied = WorkspaceService::new(
        store.clone(),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    )
    .with_matrix_advice_provider(provider.clone());
    let denied = budget_denied
        .request_engineering_advisory(&context, &request("budget-denied"))
        .await
        .unwrap();
    assert_eq!(denied.state, AdvisoryOpportunityState::NoCall);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let active = Arc::new(
        default_service.with_matrix_advisory_adapters(provider, Arc::new(AllowFixtureBudget)),
    );
    let request = request("active");
    let first = if advance_during_send {
        let active = active.clone();
        let send_context = context.clone();
        let send_request = request.clone();
        let pending = tokio::spawn(async move {
            active
                .request_engineering_advisory(&send_context, &send_request)
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(10), entered.notified())
            .await
            .expect("provider should enter after committed dispatch start");
        budget_denied
            .record_matrix_task(
                &context,
                &RecordMatrixTask {
                    task_id,
                    revision: 2,
                    expected_current_revision: 1,
                    request_id: Uuid::new_v4(),
                    input: input(),
                    choice_set: None,
                },
            )
            .await
            .unwrap();
        release.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(10), pending)
            .await
            .expect("post-send finalization should terminate")
            .unwrap()
            .unwrap()
    } else {
        active
            .request_engineering_advisory(&context, &request)
            .await
            .unwrap()
    };
    assert_eq!(
        first.state,
        if advance_during_send {
            AdvisoryOpportunityState::Invalidated
        } else {
            AdvisoryOpportunityState::Advised
        }
    );
    if advance_during_send {
        assert_eq!(
            first.primary_reason,
            AdvisoryReason::MatrixTaskRevisionChanged
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let replay = active
        .request_engineering_advisory(&context, &request)
        .await
        .unwrap();
    assert_eq!(replay.id, first.id);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let receipt = active
        .get_engineering_advisory(&context, task_id, &request.request_key)
        .await
        .unwrap();
    assert_eq!(receipt.id, first.id);
    if advance_during_send {
        assert_eq!(
            receipt.primary_reason,
            AdvisoryReason::MatrixTaskRevisionChanged
        );
        let persisted: String =
            sqlx::query_scalar("SELECT primary_reason FROM advisory_opportunity WHERE id=$1")
                .bind(first.id)
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(persisted, "matrix_task_revision_changed");
    }
    let rows: Vec<(Uuid, i32, String, String, String, Option<Vec<u8>>)> = sqlx::query_as(
        "SELECT id,attempt_number,state,send_certainty,outcome,response_payload FROM advisory_dispatch WHERE opportunity_id=$1"
    ).bind(first.id).fetch_all(pool).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (
            rows[0].1,
            rows[0].2.as_str(),
            rows[0].3.as_str(),
            rows[0].4.as_str()
        ),
        (1, "sealed", "sent", "provider_response")
    );
    let advice: Option<(Uuid, Uuid, Uuid, i64, String, Option<serde_json::Value>, String)> =
        sqlx::query_as(
            "SELECT opportunity_id,dispatch_id,task_id,matrix_task_revision,kind,ranked_choice_ids,response_payload_sha256 \
         FROM advisory_matrix_advice WHERE opportunity_id=$1",
        )
        .bind(first.id)
        .fetch_optional(pool)
        .await
        .unwrap();
    if advance_during_send {
        assert!(
            advice.is_none(),
            "stale task head must not leave usable advice"
        );
    } else {
        let advice = advice.unwrap();
        assert_eq!(
            (advice.0, advice.1, advice.2, advice.3),
            (first.id, rows[0].0, task_id, 1)
        );
        assert_eq!(advice.4, if abstain { "abstained" } else { "ranked" });
        assert_eq!(
            advice.5,
            if abstain {
                None
            } else {
                Some(serde_json::json!(["b", "a"]))
            }
        );
        let sealed_raw = rows[0].5.as_ref().expect("sealed response bytes");
        assert_eq!(advice.6, format!("{:x}", Sha256::digest(sealed_raw)));
    }
    assert_eq!(
        rows[0].5.as_deref(),
        Some(if abstain {
            b"fixture:abstained".as_slice()
        } else {
            b"fixture:ranked-b-a".as_slice()
        })
    );
    let associated: (Uuid, Uuid) =
        sqlx::query_as("SELECT tenant_id,workspace_id FROM advisory_opportunity WHERE id=$1")
            .bind(first.id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(associated, (owner.tenant_id, workspace.id));
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL 18.6 opt-in and TECT_TEST_* required"]
async fn matrix_active_ranked_abstained_replay_and_disabled_no_call() {
    let (pool, _admin_url, runtime_url) = disposable_pair().await;
    admin::migrate(&pool, "tect_ci").await.unwrap();
    case(&pool, &runtime_url, false, false).await;
    case(&pool, &runtime_url, true, false).await;
    case(&pool, &runtime_url, false, true).await;
}
