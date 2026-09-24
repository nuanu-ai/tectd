//! Synthetic application-to-PG18 Matrix dispatch proof. No external provider is contacted.

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

const SYSTEM_ID: &str = "7689197957195199396";
const DATABASE_OID: i64 = 16385;
const SOCKET: &str = "/tmp/tectd-matrix-pg18.6-fresh-tNXHgM/socket";
const PORT: u16 = 55479;
const PROFILE: &str = "synthetic-matrix-provider";
const MODEL: &str = "synthetic-matrix-model";
const POLICY: &str = "synthetic-evidence-policy/1";

#[derive(Clone, Copy, Debug)]
enum Scenario {
    Ranked,
    Abstained,
    RevokeBeforeSend,
    RevokeAfterSend,
}

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

struct Budget {
    scenario: Scenario,
    revoked: Arc<AtomicBool>,
}

#[async_trait]
impl MatrixBudgetPolicy for Budget {
    async fn authorize(
        &self,
        _: &MatrixBudgetRequest,
    ) -> tect_domain::Result<Option<MatrixBudgetAuthorization>> {
        if matches!(self.scenario, Scenario::RevokeBeforeSend) {
            self.revoked.store(true, Ordering::SeqCst);
        }
        Ok(Some(MatrixBudgetAuthorization {
            policy_id: "synthetic-budget/1".into(),
        }))
    }
}

struct Provider {
    scenario: Scenario,
    revoked: Arc<AtomicBool>,
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
        PreparedMatrixAdviceAttempt::new(
            request,
            Self::identity_value(),
            b"synthetic request".to_vec(),
        )
    }
    async fn attempt_prepared(
        &self,
        prepared: PreparedMatrixAdviceAttempt,
        _: MatrixStartedDispatchPermit,
    ) -> tect_domain::Result<MatrixProviderResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if matches!(self.scenario, Scenario::RevokeAfterSend) {
            self.revoked.store(true, Ordering::SeqCst);
        }
        let raw = b"synthetic response".to_vec();
        let ranking = if matches!(self.scenario, Scenario::Abstained) {
            MatrixRanking::Abstained {
                ranked_candidate_ids: vec![],
                recommended_candidate_id: None,
            }
        } else {
            MatrixRanking::Ranked {
                ranked_candidate_ids: vec!["a".into(), "b".into()],
                recommended_candidate_id: "a".into(),
            }
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

async fn run_case(pool: &PgPool, runtime_url: &str, scenario: Scenario) {
    let owner = admin::enroll_host(pool, None, vec![]).await.unwrap();
    let context = RequestContext {
        auth: owner.auth,
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: format!("matrix-postresponse-{}", Uuid::new_v4().simple()),
    };
    let revoked = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let service = WorkspaceService::new(
        Arc::new(PgStore::connect(runtime_url, 4).await.unwrap()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    )
    .with_matrix_evidence_validator(Arc::new(Evidence {
        revoked: revoked.clone(),
    }))
    .with_matrix_advisory_adapters(
        Arc::new(Provider {
            scenario,
            revoked: revoked.clone(),
            calls: calls.clone(),
        }),
        Arc::new(Budget {
            scenario,
            revoked: revoked.clone(),
        }),
    );
    let workspace = service
        .open_workspace(&context)
        .await
        .unwrap()
        .workspace
        .unwrap();
    let task = Uuid::new_v4();
    let input = input();
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
                input,
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
        request_key: format!("matrix-postresponse-{}", Uuid::new_v4()),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
    };
    let receipt = service
        .request_engineering_advisory(&context, &request)
        .await
        .unwrap_or_else(|error| panic!("{scenario:?} request failed: {error:?}"));
    let expected_state = if matches!(scenario, Scenario::Ranked | Scenario::Abstained) {
        AdvisoryOpportunityState::Advised
    } else {
        AdvisoryOpportunityState::Invalidated
    };
    let expected_reason = if matches!(scenario, Scenario::Ranked | Scenario::Abstained) {
        AdvisoryReason::ProviderResponse
    } else {
        AdvisoryReason::MatrixVerificationStale
    };
    assert_eq!(
        (receipt.state, receipt.primary_reason),
        (expected_state, expected_reason)
    );
    let saved = service
        .get_engineering_advisory(&context, task, &request.request_key)
        .await
        .unwrap();
    assert_eq!(
        (
            saved.opportunity.id,
            saved.opportunity.state,
            saved.opportunity.primary_reason,
        ),
        (receipt.id, expected_state, expected_reason)
    );
    let replay = service
        .request_engineering_advisory(&context, &request)
        .await
        .unwrap();
    assert_eq!(
        (replay.id, replay.state, replay.primary_reason),
        (receipt.id, expected_state, expected_reason)
    );
    let expected_calls = if matches!(scenario, Scenario::RevokeBeforeSend) {
        0
    } else {
        1
    };
    assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
    let dispatches: Vec<(String, String, Option<String>, Option<Vec<u8>>)> = sqlx::query_as(
        "SELECT state,send_certainty,outcome,response_payload FROM advisory_dispatch WHERE opportunity_id=$1")
        .bind(receipt.id).fetch_all(pool).await.unwrap();
    assert_eq!(dispatches.len(), 1);
    match scenario {
        Scenario::RevokeBeforeSend => assert_eq!(
            dispatches[0],
            ("cancelled".into(), "not_sent".into(), None, None)
        ),
        _ => assert_eq!(
            dispatches[0],
            (
                "sealed".into(),
                "sent".into(),
                Some("provider_response".into()),
                Some(b"synthetic response".to_vec())
            )
        ),
    }
    let advice: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_matrix_advice WHERE opportunity_id=$1")
            .bind(receipt.id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(
        advice,
        if matches!(scenario, Scenario::Ranked | Scenario::Abstained) {
            1
        } else {
            0
        }
    );
    let advice_kind: Option<String> =
        sqlx::query_scalar("SELECT kind FROM advisory_matrix_advice WHERE opportunity_id=$1")
            .bind(receipt.id)
            .fetch_optional(pool)
            .await
            .unwrap();
    match scenario {
        Scenario::Ranked => assert_eq!(advice_kind.as_deref(), Some("ranked")),
        Scenario::Abstained => assert_eq!(advice_kind.as_deref(), Some("abstained")),
        Scenario::RevokeBeforeSend | Scenario::RevokeAfterSend => assert_eq!(advice_kind, None),
    }
}

#[tokio::test]
#[ignore = "writes only the exact disposable PostgreSQL 18.6 fixture"]
async fn verified_matrix_postresponse_is_terminal_and_replay_does_not_resend() {
    let (pool, runtime_url) = disposable_pair().await;
    admin::migrate(&pool, "tect_ci").await.unwrap();
    for scenario in [
        Scenario::Ranked,
        Scenario::Abstained,
        Scenario::RevokeBeforeSend,
        Scenario::RevokeAfterSend,
    ] {
        run_case(&pool, &runtime_url, scenario).await;
    }
}
