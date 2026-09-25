use async_trait::async_trait;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgConnectOptions};
use std::{
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tect_application::{
    MatrixAdviceProvider, MatrixBudgetAuthorization, MatrixBudgetPolicy, MatrixBudgetRequest,
    MatrixEvidenceValidator, MatrixProviderIdentity, MatrixProviderRequest, MatrixProviderResponse,
    MatrixStartedDispatchPermit, PreparedMatrixAdviceAttempt, RecordMatrixTask,
    RequestEngineeringAdvisory, WorkspaceService,
};
use tect_domain::{
    AdvisoryModelConfiguration, AdvisoryOpportunityState, AdvisoryProviderProfileRef,
    AdvisoryReason, AdvisoryRequestPreference, ConfigureWorkspaceAdvisory, EngineeringCandidate,
    EngineeringChoiceSet, EngineeringMatrixInput, EvidenceValidationOutcome,
    MATRIX_CHOICE_SET_SCHEMA, MATRIX_VERIFICATION_SCHEMA, MatrixEvidenceBinding, MatrixRanking,
    MatrixVerificationRecord, RequestContext, RequiredMatrixFact, WorkspaceAdvisoryMode,
    required_matrix_facts,
};
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

const SYSTEM_ID: &str = "7689209824515417725";
const DATABASE_OID: i64 = 16385;
const SOCKET: &str = "/tmp/tectd-matrix-replay-tUv9UZ";
const PORT: u16 = 55480;
const PROFILE: &str = "synthetic-matrix-provider";
const MODEL: &str = "synthetic-matrix-model";
const POLICY: &str = "synthetic-evidence-policy/1";
static TEST_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Evidence {
    revoked: Arc<AtomicBool>,
}

#[async_trait]
impl MatrixEvidenceValidator for Evidence {
    fn policy_version(&self) -> &str {
        POLICY
    }
    async fn validate(
        &self,
        _: Uuid,
        _: Uuid,
        _: i64,
        _: &RequiredMatrixFact,
        _: &str,
        _: i64,
    ) -> tect_domain::Result<MatrixEvidenceBinding> {
        unreachable!("fixture inserts verification")
    }
    async fn revalidate(
        &self,
        _: Uuid,
        _: Uuid,
        _: i64,
        fact: &RequiredMatrixFact,
        binding: &MatrixEvidenceBinding,
        now: i64,
    ) -> tect_domain::Result<()> {
        if self.revoked.load(Ordering::SeqCst)
            || binding.fact_path != fact.path
            || binding.value_digest != fact.value_digest
            || binding.expires_at <= now
        {
            return Err(tect_domain::Error::Forbidden);
        }
        Ok(())
    }
}

struct Budget;

#[async_trait]
impl MatrixBudgetPolicy for Budget {
    async fn authorize(
        &self,
        _: &MatrixBudgetRequest,
        policy: &tect_domain::AdvisoryBudgetPolicy,
    ) -> tect_domain::Result<Option<MatrixBudgetAuthorization>> {
        Ok(Some(MatrixBudgetAuthorization {
            policy_id: policy.id().to_string(),
        }))
    }
}

struct Provider {
    calls: Arc<AtomicUsize>,
}

impl Provider {
    fn identity_value() -> MatrixProviderIdentity {
        MatrixProviderIdentity {
            provider_profile_ref: AdvisoryProviderProfileRef { id: PROFILE.into() },
            model_configuration: AdvisoryModelConfiguration {
                model: MODEL.into(),
            },
            destination: "https://synthetic.invalid/matrix".into(),
            wire_version: "synthetic-matrix/1".into(),
        }
    }
}

#[async_trait]
impl MatrixAdviceProvider for Provider {
    fn identity(&self) -> Option<MatrixProviderIdentity> {
        Some(Self::identity_value())
    }
    fn prepare(
        &self,
        request: &MatrixProviderRequest,
    ) -> tect_domain::Result<PreparedMatrixAdviceAttempt> {
        let binding = request.binding();
        let mut saved_binding = serde_json::json!({
            "task_id": binding.task_id.to_string(),
            "task_revision": binding.task_revision.to_string(),
            "input_digest": binding.input_digest,
            "choice_set_id": binding.choice_set_id,
            "choice_set_version": binding.choice_set_version,
            "choice_set_digest": binding.choice_set_digest,
            "evaluation_digest": binding.evaluation_digest,
        });
        if let Some(digest) = &binding.verification_digest {
            saved_binding["verification_digest"] = serde_json::json!(digest);
        }
        let body = serde_json::to_vec(&serde_json::json!({
            "model": MODEL,
            "state": {"binding": saved_binding},
        }))
        .unwrap();
        PreparedMatrixAdviceAttempt::new(request, Self::identity_value(), body)
    }
    fn parse_sealed_response(
        &self,
        request: &MatrixProviderRequest,
        saved: &tect_application::StoredMatrixDispatch,
    ) -> tect_domain::Result<MatrixProviderResponse> {
        let raw = saved
            .response_payload
            .clone()
            .ok_or(tect_domain::Error::InputConflict)?;
        let ranking = MatrixRanking::Ranked {
            ranked_candidate_ids: vec!["a".into(), "b".into()],
            recommended_candidate_id: "a".into(),
        };
        Ok(MatrixProviderResponse {
            binding: request.binding().clone(),
            provider_profile_ref: Self::identity_value().provider_profile_ref,
            model_configuration: Self::identity_value().model_configuration,
            response_payload_sha256: format!("{:x}", Sha256::digest(&raw)),
            raw_response_payload: raw,
            ranking,
            input_tokens: Some(3),
            output_tokens: Some(4),
        })
    }
    async fn attempt_prepared(
        &self,
        prepared: PreparedMatrixAdviceAttempt,
        _: MatrixStartedDispatchPermit,
    ) -> tect_domain::Result<MatrixProviderResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let raw = b"synthetic response".to_vec();
        let ranking = MatrixRanking::Ranked {
            ranked_candidate_ids: vec!["a".into(), "b".into()],
            recommended_candidate_id: "a".into(),
        };
        Ok(MatrixProviderResponse {
            binding: prepared.binding().clone(),
            provider_profile_ref: Self::identity_value().provider_profile_ref,
            model_configuration: Self::identity_value().model_configuration,
            response_payload_sha256: format!("{:x}", Sha256::digest(&raw)),
            raw_response_payload: raw,
            ranking,
            input_tokens: Some(3),
            output_tokens: Some(4),
        })
    }
}

async fn disposable_pair() -> (PgPool, String) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    assert_eq!(
        std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").as_deref(),
        Ok(SYSTEM_ID)
    );
    assert_eq!(
        std::env::var("TECT_TEST_EXPECTED_DB_OID").as_deref(),
        Ok("16385")
    );
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    assert_eq!(
        std::env::var("TECT_TEST_RUNTIME_ROLE").as_deref(),
        Ok("tect_ci")
    );
    let admin = PgConnectOptions::from_str(&admin_url).unwrap();
    let runtime = PgConnectOptions::from_str(&runtime_url).unwrap();
    for (options, user) in [(&admin, "postgres"), (&runtime, "tect_ci")] {
        assert_eq!(options.get_username(), user);
        assert_eq!(options.get_database(), Some("tect_test"));
        assert_eq!(options.get_socket().and_then(|p| p.to_str()), Some(SOCKET));
        assert_eq!(options.get_port(), PORT);
    }
    let pool = PgPool::connect_with(admin).await.unwrap();
    let identity: (i32, String, String, i64, String) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),current_user,\
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_control_system())",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        identity,
        (
            180006,
            "tect_test".into(),
            "postgres".into(),
            DATABASE_OID,
            SYSTEM_ID.into()
        )
    );
    let runtime_pool = PgPool::connect_with(runtime).await.unwrap();
    let runtime_identity: (String, String, i64) = sqlx::query_as(
        "SELECT current_database(),current_user,(SELECT oid::bigint FROM pg_database WHERE datname=current_database())")
        .fetch_one(&runtime_pool).await.unwrap();
    assert_eq!(
        runtime_identity,
        ("tect_test".into(), "tect_ci".into(), DATABASE_OID)
    );
    (pool, runtime_url)
}

fn input() -> EngineeringMatrixInput {
    serde_json::from_value(serde_json::json!({
        "mode":{"state":"known","value":"demo","provenance":"synthetic owner"},
        "envelope":{"scale":{"state":"known","value":"one request","provenance":"synthetic owner"},
          "operational_facts":{"state":"known_empty","provenance":"synthetic owner"}},
        "criticality":{"state":"known","value":"low","provenance":"synthetic owner"},
        "intent":{"state":"known","value":{"kind":"other","description":"demo"},"provenance":"synthetic owner"},
        "urgency":{"state":"known","value":"ordinary","provenance":"synthetic owner"},
        "promised_behavior":{"state":"known","value":"demo","provenance":"synthetic owner"},
        "promised_proof":{"state":"known","value":"check","provenance":"synthetic owner"},
        "affected_guarantees":{"state":"known_empty","provenance":"synthetic owner"},
        "actual_exposure":{"state":"known","value":false,"provenance":"synthetic owner"},
        "demand_commitment":{"state":"known","value":"no_commitment","provenance":"synthetic owner"},
        "latency_commitment":{"state":"known","value":"no_commitment","provenance":"synthetic owner"},
        "urgent_repair":{"state":"known","value":false,"provenance":"synthetic owner"}
    })).unwrap()
}

async fn install_verification(
    pool: &PgPool,
    runtime_url: &str,
    tenant: Uuid,
    workspace: Uuid,
    task: Uuid,
    owner: Uuid,
    revision: &tect_application::MatrixTaskRevision,
) {
    let verifier = Uuid::new_v4();
    let host = Uuid::new_v4();
    let session = Uuid::new_v4();
    sqlx::query("INSERT INTO principals (id,tenant_id,role) VALUES ($1,$2,'verifier')")
        .bind(verifier)
        .bind(tenant)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO hosts (id,tenant_id,principal_id,credential_digest) VALUES ($1,$2,$3,$4)",
    )
    .bind(host)
    .bind(tenant)
    .bind(verifier)
    .bind(format!("{:x}", Sha256::digest(host.to_string())))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO memberships (tenant_id,workspace_id,principal_id) VALUES ($1,$2,$3)")
        .bind(tenant)
        .bind(workspace)
        .bind(verifier)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO agent_sessions (id,tenant_id,host_id,workspace_id,native_session_id) VALUES ($1,$2,$3,$4,$5)")
        .bind(session).bind(tenant).bind(host).bind(workspace).bind(Uuid::new_v4().to_string())
        .execute(pool).await.unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let bindings: Vec<_> = required_matrix_facts(&revision.input)
        .unwrap()
        .into_iter()
        .map(|fact| MatrixEvidenceBinding {
            fact_path: fact.path,
            value_digest: fact.value_digest,
            evidence_ref: "synthetic-ref".into(),
            content_digest: format!("{:x}", Sha256::digest(b"synthetic-ref")),
            source: "synthetic".into(),
            subject: task.to_string(),
            observed_at: now - 10,
            expires_at: now + 3600,
            validation_outcome: EvidenceValidationOutcome::Accepted,
        })
        .collect();
    let mut record = MatrixVerificationRecord {
        schema: MATRIX_VERIFICATION_SCHEMA.into(),
        task_id: task.to_string(),
        task_revision: "1".into(),
        input_digest: revision.input_digest.clone(),
        owner_principal: owner.to_string(),
        verifier_principal: verifier.to_string(),
        policy_version: POLICY.into(),
        bindings,
        digest: String::new(),
    };
    record.digest = record.canonical_digest().unwrap();
    let runtime = PgPool::connect(runtime_url).await.unwrap();
    let mut tx = runtime.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let verification_id: Uuid = sqlx::query_scalar("INSERT INTO matrix_verifications (tenant_id,workspace_id,task_id,task_revision,input_digest,schema,owner_principal_id,verifier_principal_id,verifier_session_id,verification_reason,policy_version,record_digest) VALUES ($1,$2,$3,1,$4,$5,$6,$7,$8,'matrix_facts_verified',$9,$10) RETURNING id")
        .bind(tenant).bind(workspace).bind(task).bind(&record.input_digest).bind(&record.schema)
        .bind(owner).bind(verifier).bind(session).bind(&record.policy_version).bind(&record.digest)
        .fetch_one(&mut *tx).await.unwrap();
    for binding in &record.bindings {
        sqlx::query("INSERT INTO matrix_verification_bindings (tenant_id,workspace_id,verification_id,fact_path,value_digest,evidence_ref,content_digest,source,subject,observed_at,expires_at,validation_outcome) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'accepted')")
            .bind(tenant).bind(workspace).bind(verification_id).bind(&binding.fact_path)
            .bind(&binding.value_digest).bind(&binding.evidence_ref).bind(&binding.content_digest)
            .bind(&binding.source).bind(&binding.subject).bind(binding.observed_at).bind(binding.expires_at)
            .execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
}

// A separate disposable cluster is used for these interruption tests. The
// fixture is created through the same public application API as a real call.
async fn replay_fixture(
    pool: &PgPool,
    runtime_url: &str,
    provider: Arc<dyn MatrixAdviceProvider>,
    revoked: Arc<AtomicBool>,
) -> (
    Arc<WorkspaceService>,
    RequestContext,
    RequestEngineeringAdvisory,
) {
    let owner = admin::enroll_host(pool, None, vec![]).await.unwrap();
    let context = RequestContext {
        auth: owner.auth,
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: format!("matrix-replay-{}", Uuid::new_v4().simple()),
    };
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(PgStore::connect(runtime_url, 4).await.unwrap()),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(Evidence {
            revoked: revoked.clone(),
        }))
        .with_matrix_advisory_adapters(provider, Arc::new(Budget)),
    );
    let workspace = service
        .open_workspace(&context)
        .await
        .unwrap()
        .workspace
        .unwrap();
    let task = Uuid::new_v4();
    let choice = EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: format!("choice-{}", Uuid::new_v4().simple()),
        version: 1,
        task_id: task.to_string(),
        task_revision: "1".into(),
        decision_question: "Which synthetic approach?".into(),
        candidates: ["a", "b"]
            .into_iter()
            .map(|id| EngineeringCandidate {
                candidate_id: id.into(),
                title: format!("Approach {id}"),
                approach: format!("Synthetic approach {id}"),
                assumption_fact_ids: vec![],
            })
            .collect(),
    };
    let revision = service
        .record_matrix_task(
            &context,
            &RecordMatrixTask {
                task_id: task,
                revision: 1,
                expected_current_revision: 0,
                request_id: Uuid::new_v4(),
                input: input(),
                choice_set: Some(choice),
            },
        )
        .await
        .unwrap();
    install_verification(
        pool,
        runtime_url,
        owner.tenant_id,
        workspace.id,
        task,
        owner.principal_id,
        &revision,
    )
    .await;
    service
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
    let request = RequestEngineeringAdvisory {
        task_id: task,
        expected_task_revision: 1,
        request_key: format!("matrix-replay-{}", Uuid::new_v4().simple()),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
    };
    (service, context, request)
}

async fn wait_dispatch(
    pool: &PgPool,
    request_key: &str,
    wanted: &str,
) -> (Uuid, Uuid, String, String, Option<String>) {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let row: Option<(Uuid, Uuid, String, String, Option<String>)> = sqlx::query_as(
                "SELECT d.id,o.id,d.state,d.send_certainty,d.outcome \
                 FROM advisory_dispatch d JOIN advisory_opportunity o ON o.id=d.opportunity_id \
                 WHERE o.request_key=$1",
            )
            .bind(request_key)
            .fetch_optional(pool)
            .await
            .unwrap();
            if let Some(row) = row.filter(|row| row.2 == wanted) {
                return row;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("dispatch did not reach expected crash window")
}

struct BarrierProvider {
    inner: Provider,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
    attempts: Arc<AtomicUsize>,
    parses: Arc<AtomicUsize>,
}

#[async_trait]
impl MatrixAdviceProvider for BarrierProvider {
    fn identity(&self) -> Option<MatrixProviderIdentity> {
        self.inner.identity()
    }
    fn prepare(
        &self,
        request: &MatrixProviderRequest,
    ) -> tect_domain::Result<PreparedMatrixAdviceAttempt> {
        self.inner.prepare(request)
    }
    fn parse_sealed_response(
        &self,
        request: &MatrixProviderRequest,
        saved: &tect_application::StoredMatrixDispatch,
    ) -> tect_domain::Result<MatrixProviderResponse> {
        self.parses.fetch_add(1, Ordering::SeqCst);
        self.inner.parse_sealed_response(request, saved)
    }
    async fn attempt_prepared(
        &self,
        prepared: PreparedMatrixAdviceAttempt,
        permit: MatrixStartedDispatchPermit,
    ) -> tect_domain::Result<MatrixProviderResponse> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        self.release.notified().await;
        self.inner.attempt_prepared(prepared, permit).await
    }
}

async fn authorized_interruption(pool: &PgPool, runtime_url: &str, stale: bool) {
    let revoked = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let provider: Arc<dyn MatrixAdviceProvider> = Arc::new(Provider {
        calls: calls.clone(),
    });
    let (service, context, request) =
        replay_fixture(pool, runtime_url, provider, revoked.clone()).await;
    let nonce = Uuid::new_v4().simple().to_string();
    let function = format!("matrix_replay_pause_{nonce}");
    let trigger = format!("matrix_replay_trigger_{nonce}");
    let key = i64::from_str_radix(&nonce[..15], 16).unwrap();
    // This DDL exists only in the independently initialized disposable DB.
    // Its predicate is the random request key for this one fixture.
    let create_function = format!(
        "CREATE FUNCTION {function}() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN IF NEW.state='sending' AND EXISTS \
         (SELECT 1 FROM advisory_opportunity WHERE id=NEW.opportunity_id \
          AND request_key='{}') THEN PERFORM pg_advisory_xact_lock({key}); \
         END IF; RETURN NEW; END $$",
        request.request_key
    );
    sqlx::query(&create_function).execute(pool).await.unwrap();
    sqlx::query(&format!(
        "CREATE TRIGGER {trigger} BEFORE UPDATE OF state ON advisory_dispatch \
         FOR EACH ROW EXECUTE FUNCTION {function}()"
    ))
    .execute(pool)
    .await
    .unwrap();
    let mut blocker = pool.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(key)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let service_task = service.clone();
    let context_task = context.clone();
    let request_task = request.clone();
    let running = tokio::spawn(async move {
        service_task
            .request_engineering_advisory(&context_task, &request_task)
            .await
    });
    let (dispatch_id, opportunity_id, _, certainty, _) =
        wait_dispatch(pool, &request.request_key, "authorized").await;
    assert_eq!(certainty, "not_sent");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    running.abort();
    let _ = running.await;
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(key)
        .execute(&mut *blocker)
        .await
        .unwrap();
    drop(blocker);
    sqlx::query(&format!("DROP TRIGGER {trigger} ON advisory_dispatch"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(&format!("DROP FUNCTION {function}()"))
        .execute(pool)
        .await
        .unwrap();
    if stale {
        revoked.store(true, Ordering::SeqCst);
    }
    let result = service
        .request_engineering_advisory(&context, &request)
        .await;
    if stale {
        let receipt = result.expect("stale authorized replay must cancel");
        assert_eq!(receipt.state, AdvisoryOpportunityState::Invalidated);
        assert_eq!(
            receipt.primary_reason,
            AdvisoryReason::MatrixVerificationStale
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let replay = service
            .request_engineering_advisory(&context, &request)
            .await
            .unwrap();
        assert_eq!(replay.id, opportunity_id);
        assert_eq!(replay.state, AdvisoryOpportunityState::Invalidated);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    } else {
        let receipt = result.unwrap();
        assert_eq!(receipt.id, opportunity_id);
        assert_eq!(receipt.state, AdvisoryOpportunityState::Advised);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let replay = service
            .request_engineering_advisory(&context, &request)
            .await
            .unwrap();
        assert_eq!(replay.id, receipt.id);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    let actual: (Uuid, String, String) =
        sqlx::query_as("SELECT id,state,send_certainty FROM advisory_dispatch WHERE id=$1")
            .bind(dispatch_id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(actual.0, dispatch_id);
    assert_eq!(actual.1, if stale { "cancelled" } else { "sealed" });
    assert_eq!(actual.2, if stale { "not_sent" } else { "sent" });
}

#[tokio::test]
#[ignore = "writes only the dedicated disposable PG18 replay cluster"]
async fn authorized_replay_uses_same_dispatch_once() {
    let _serial = TEST_MUTEX.lock().await;
    let (pool, runtime_url) = disposable_pair().await;
    authorized_interruption(&pool, &runtime_url, false).await;
}

#[tokio::test]
#[ignore = "writes only the dedicated disposable PG18 replay cluster"]
async fn authorized_replay_cancels_stale_source_without_send() {
    let _serial = TEST_MUTEX.lock().await;
    let (pool, runtime_url) = disposable_pair().await;
    authorized_interruption(&pool, &runtime_url, true).await;
}

#[tokio::test]
#[ignore = "writes only the dedicated disposable PG18 replay cluster"]
async fn sealed_response_replays_saved_bytes_without_provider_send() {
    let _serial = TEST_MUTEX.lock().await;
    let (pool, runtime_url) = disposable_pair().await;
    let revoked = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let attempts = Arc::new(AtomicUsize::new(0));
    let parses = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let provider: Arc<dyn MatrixAdviceProvider> = Arc::new(BarrierProvider {
        inner: Provider {
            calls: calls.clone(),
        },
        entered: entered.clone(),
        release: release.clone(),
        attempts: attempts.clone(),
        parses: parses.clone(),
    });
    let (service, context, request) = replay_fixture(&pool, &runtime_url, provider, revoked).await;
    let service_task = service.clone();
    let context_task = context.clone();
    let request_task = request.clone();
    let running = tokio::spawn(async move {
        service_task
            .request_engineering_advisory(&context_task, &request_task)
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(15), entered.notified())
        .await
        .expect("provider did not reach committed Sending state");
    let (dispatch_id, opportunity_id, _, certainty, _) =
        wait_dispatch(&pool, &request.request_key, "sending").await;
    assert_eq!(certainty, "sent_unknown");
    let mut lock = pool.begin().await.unwrap();
    let locked: Uuid =
        sqlx::query_scalar("SELECT id FROM advisory_opportunity WHERE id=$1 FOR UPDATE")
            .bind(opportunity_id)
            .fetch_one(&mut *lock)
            .await
            .unwrap();
    assert_eq!(locked, opportunity_id);
    release.notify_one();
    let (saved_id, saved_opportunity, _, saved_certainty, outcome) =
        wait_dispatch(&pool, &request.request_key, "sealed").await;
    assert_eq!((saved_id, saved_opportunity), (dispatch_id, opportunity_id));
    assert_eq!(saved_certainty, "sent");
    assert_eq!(outcome.as_deref(), Some("provider_response"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    running.abort();
    let _ = running.await;
    lock.rollback().await.unwrap();
    let receipt = service
        .request_engineering_advisory(&context, &request)
        .await
        .unwrap();
    assert_eq!(
        (receipt.id, receipt.state),
        (opportunity_id, AdvisoryOpportunityState::Advised)
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(parses.load(Ordering::SeqCst), 1);
    let advice_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_matrix_advice WHERE opportunity_id=$1 AND dispatch_id=$2",
    )
    .bind(opportunity_id)
    .bind(dispatch_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(advice_count, 1);
    let replay = service
        .request_engineering_advisory(&context, &request)
        .await
        .unwrap();
    assert_eq!(replay.id, receipt.id);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(parses.load(Ordering::SeqCst), 1);
}

#[tokio::test]
#[ignore = "writes only the dedicated disposable PG18 replay cluster"]
async fn sending_unknown_replay_does_not_send_again() {
    let _serial = TEST_MUTEX.lock().await;
    let (pool, runtime_url) = disposable_pair().await;
    let revoked = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let attempts = Arc::new(AtomicUsize::new(0));
    let parses = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(tokio::sync::Notify::new());
    let provider: Arc<dyn MatrixAdviceProvider> = Arc::new(BarrierProvider {
        inner: Provider {
            calls: calls.clone(),
        },
        entered: entered.clone(),
        release: Arc::new(tokio::sync::Notify::new()),
        attempts: attempts.clone(),
        parses: parses.clone(),
    });
    let (service, context, request) = replay_fixture(&pool, &runtime_url, provider, revoked).await;
    let service_task = service.clone();
    let context_task = context.clone();
    let request_task = request.clone();
    let running = tokio::spawn(async move {
        service_task
            .request_engineering_advisory(&context_task, &request_task)
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(15), entered.notified())
        .await
        .expect("provider did not reach committed Sending state");
    let (dispatch_id, opportunity_id, _, certainty, outcome) =
        wait_dispatch(&pool, &request.request_key, "sending").await;
    assert_eq!(certainty, "sent_unknown");
    assert_eq!(outcome, None);
    running.abort();
    let _ = running.await;
    let replay = service
        .request_engineering_advisory(&context, &request)
        .await
        .unwrap();
    assert_eq!(replay.id, opportunity_id);
    assert_eq!(replay.state, AdvisoryOpportunityState::AwaitingResponse);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(parses.load(Ordering::SeqCst), 0);
    let stored: (String, String) =
        sqlx::query_as("SELECT state,send_certainty FROM advisory_dispatch WHERE id=$1")
            .bind(dispatch_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, ("sending".into(), "sent_unknown".into()));
}
