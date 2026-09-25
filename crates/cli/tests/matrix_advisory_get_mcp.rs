//! Public MCP proof for a ranked Matrix read. The provider and evidence source are synthetic.
//! This ignored test writes only to the pinned disposable PostgreSQL 18.6 fixture.
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use async_trait::async_trait;
use recovery_support::{Mcp, host_file, private_temp};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgConnectOptions};
use std::{
    os::unix::fs::PermissionsExt,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use support::route;
use tect_application::{
    MatrixAdviceProvider, MatrixBudgetAuthorization, MatrixBudgetPolicy, MatrixBudgetRequest,
    MatrixEvidenceValidator, MatrixProviderIdentity, MatrixProviderRequest, MatrixProviderResponse,
    MatrixStartedDispatchPermit, PreparedMatrixAdviceAttempt, WorkspaceService,
};
use tect_domain::{
    AdvisoryModelConfiguration, AdvisoryProviderProfileRef, EngineeringMatrixInput,
    EvidenceValidationOutcome, MatrixEvidenceBinding, MatrixRanking, RequiredMatrixFact, Result,
    required_matrix_facts,
};
use tect_postgres::{PgStore, admin};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};
use uuid::Uuid;

const SOCKET: &str = "/tmp/tectd-matrix-pg18.6-fresh-tNXHgM/socket";
const PORT: u16 = 55479;
const SYSTEM_ID: &str = "7689197957195199396";
const DATABASE_OID: i64 = 16385;
const PROFILE: &str = "synthetic-matrix-provider";
const MODEL: &str = "synthetic-matrix-model";

struct Evidence {
    revoked: Arc<AtomicBool>,
}

#[async_trait]
impl MatrixEvidenceValidator for Evidence {
    fn policy_version(&self) -> &str {
        "synthetic-mcp-policy/1"
    }
    async fn validate(
        &self,
        _: Uuid,
        task: Uuid,
        _: i64,
        fact: &RequiredMatrixFact,
        reference: &str,
        now: i64,
    ) -> Result<MatrixEvidenceBinding> {
        Ok(MatrixEvidenceBinding {
            fact_path: fact.path.clone(),
            value_digest: fact.value_digest.clone(),
            evidence_ref: reference.into(),
            content_digest: format!("{:x}", Sha256::digest(reference.as_bytes())),
            source: "synthetic-mcp-evidence".into(),
            subject: task.to_string(),
            observed_at: now - 10,
            expires_at: now + 3600,
            validation_outcome: EvidenceValidationOutcome::Accepted,
        })
    }
    async fn revalidate(
        &self,
        _: Uuid,
        _: Uuid,
        _: i64,
        fact: &RequiredMatrixFact,
        binding: &MatrixEvidenceBinding,
        now: i64,
    ) -> Result<()> {
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
    ) -> Result<Option<MatrixBudgetAuthorization>> {
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
    fn prepare(&self, request: &MatrixProviderRequest) -> Result<PreparedMatrixAdviceAttempt> {
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
    ) -> Result<MatrixProviderResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let raw = b"synthetic private response bytes".to_vec();
        Ok(MatrixProviderResponse {
            binding: prepared.binding().clone(),
            provider_profile_ref: Self::identity_value().provider_profile_ref,
            model_configuration: Self::identity_value().model_configuration,
            response_payload_sha256: format!("{:x}", Sha256::digest(&raw)),
            raw_response_payload: raw,
            ranking: MatrixRanking::Ranked {
                ranked_candidate_ids: vec!["a".into(), "b".into()],
                recommended_candidate_id: "a".into(),
            },
            input_tokens: Some(3),
            output_tokens: Some(4),
        })
    }
}

fn input() -> Value {
    json!({
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
    })
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
    assert_eq!(
        std::env::var("TECT_TEST_RUNTIME_ROLE").as_deref(),
        Ok("tect_ci")
    );
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let admin_options = PgConnectOptions::from_str(&admin_url).unwrap();
    let runtime_options = PgConnectOptions::from_str(&runtime_url).unwrap();
    for (options, user) in [(&admin_options, "postgres"), (&runtime_options, "tect_ci")] {
        assert_eq!(options.get_username(), user);
        assert_eq!(options.get_database(), Some("tect_test"));
        assert_eq!(
            options.get_socket().and_then(|path| path.to_str()),
            Some(SOCKET)
        );
        assert_eq!(options.get_port(), PORT);
    }
    let pool = PgPool::connect_with(admin_options).await.unwrap();
    let identity: (i32, String, String, i64, String, i64) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),current_user,\
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_control_system()),\
         (SELECT max(version) FROM _sqlx_migrations)",
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
            SYSTEM_ID.into(),
            57
        )
    );
    let runtime = PgPool::connect_with(runtime_options).await.unwrap();
    let runtime_identity: (String, String, i64) = sqlx::query_as(
        "SELECT current_database(),current_user,(SELECT oid::bigint FROM pg_database WHERE datname=current_database())"
    ).fetch_one(&runtime).await.unwrap();
    assert_eq!(
        runtime_identity,
        ("tect_test".into(), "tect_ci".into(), DATABASE_OID)
    );
    (pool, runtime_url)
}

async fn bounded_get(
    socket: &std::path::Path,
    context: &tect_domain::RequestContext,
    task: Uuid,
    key: &str,
) -> Value {
    let mut stream = UnixStream::connect(socket).await.unwrap();
    let request = json!({"api_version":2,"context":context,"tool_name":"get_engineering_advisory",
        "arguments":{"task_id":task,"request_key":key},"output_capacity":1});
    stream
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    stream.shutdown().await.unwrap();
    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .await
        .unwrap();
    serde_json::from_str(&response).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "writes only the pinned disposable PostgreSQL 18.6 fixture"]
async fn public_mcp_ranked_advice_get_is_current_typed_and_capacity_bounded() {
    let (pool, runtime_url) = disposable_pair().await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("matrix-advice.sock");
    let revoked = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap()),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(Evidence {
            revoked: revoked.clone(),
        }))
        .with_matrix_advisory_adapters(
            Arc::new(Provider {
                calls: calls.clone(),
            }),
            Arc::new(Budget),
        ),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));

    let enrolled = admin::enroll_host(&pool, None, vec![]).await.unwrap();
    let owner_config = root.join("owner.json");
    host_file(&owner_config, &enrolled.auth);
    let workspace_key = format!("mcp-matrix-advice-{}", Uuid::new_v4());
    let owner_native = Uuid::new_v4().to_string();
    let mut owner = Mcp::start(&socket, &owner_config, &owner_native, &workspace_key).await;
    let tools = owner.exchange("tools/list", json!({})).await;
    let names: Vec<_> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["get_state", "query", "command", "execute", "help"]);
    let opened = owner.call("open_workspace", json!({})).await;
    let workspace = Uuid::parse_str(opened["workspace"]["id"].as_str().unwrap()).unwrap();
    let task = Uuid::new_v4();
    let choice = json!({"schema":"tect.matrix-choice-set/1","choice_set_id":"synthetic-choice",
    "version":1,"task_id":task.to_string(),"task_revision":"1",
    "decision_question":"Which synthetic approach?","candidates":[
        {"candidate_id":"a","title":"Approach a","approach":"Synthetic approach a","assumption_fact_ids":[]},
        {"candidate_id":"b","title":"Approach b","approach":"Synthetic approach b","assumption_fact_ids":[]}
    ]});
    let recorded = route(
        &mut owner,
        "command",
        "task.source.record",
        json!({
            "task_id":task,"revision":1,"expected_current_revision":0,
            "request_id":Uuid::new_v4(),"input":input(),"choice_set":choice
        }),
    )
    .await;
    assert_eq!(recorded["revision"], 1);
    let parsed: EngineeringMatrixInput = serde_json::from_value(recorded["input"].clone()).unwrap();
    let evidence: Vec<_> = required_matrix_facts(&parsed)
        .unwrap()
        .iter()
        .map(|fact| {
            json!({
                "fact_path":fact.path,"evidence_ref":format!("urn:synthetic:{}:1", fact.path)
            })
        })
        .collect();
    let verifier = admin::prepare_verifier_enrollment(&pool, enrolled.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let verifier_config = root.join("verifier.json");
    host_file(&verifier_config, &verifier.auth);
    let mut independent = Mcp::start(
        &socket,
        &verifier_config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    independent.call("open_workspace", json!({})).await;
    let verified = route(
        &mut independent,
        "command",
        "engineering.matrix.verify",
        json!({
            "task_id":task,"expected_revision":1,"input_digest":recorded["input_digest"],
            "evidence":evidence
        }),
    )
    .await;
    assert_eq!(
        verified["facts"].as_array().unwrap().len(),
        required_matrix_facts(&parsed).unwrap().len()
    );
    let configured = route(
        &mut owner,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":0,"mode":"optional","provider_profile_ref":{"id":PROFILE},
            "model_configuration":{"model":MODEL}
        }),
    )
    .await;
    assert_eq!(configured["revision"], 1);
    let key = format!("mcp-ranked-{}", Uuid::new_v4());
    let requested = route(
        &mut owner,
        "command",
        "engineering.advisory.request",
        json!({
            "task_id":task,"expected_task_revision":1,"request_key":key
        }),
    )
    .await;
    assert_eq!(requested["state"], "advised", "{requested}");
    assert_eq!(requested["provider_called"], true);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let current = route(
        &mut owner,
        "query",
        "engineering.advisory.get",
        json!({
            "task_id":task,"request_key":key
        }),
    )
    .await;
    assert_eq!(current["opportunity_id"], requested["opportunity_id"]);
    assert_eq!(
        current["current_advice"]["outcome"]["status"], "ranked",
        "{current}"
    );
    assert_eq!(
        current["current_advice"]["outcome"]["ranked_choice_ids"],
        json!(["a", "b"])
    );
    assert_eq!(
        current["current_advice"]["verification_digest"],
        verified["verification_digest"]
    );
    assert!(
        !current
            .to_string()
            .contains("synthetic private response bytes")
    );
    assert!(!current.to_string().contains("raw_response_payload"));
    let advice_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_matrix_advice WHERE opportunity_id=$1")
            .bind(Uuid::parse_str(requested["opportunity_id"].as_str().unwrap()).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(advice_count, 1);
    let context = tect_domain::RequestContext {
        auth: enrolled.auth.clone(),
        native_session_id: owner_native,
        workspace_key: workspace_key.clone(),
    };
    let tiny = bounded_get(&socket, &context, task, &key).await;
    assert_eq!(tiny["error"], "request_too_large");
    let advice_after_tiny: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_matrix_advice WHERE opportunity_id=$1")
            .bind(Uuid::parse_str(requested["opportunity_id"].as_str().unwrap()).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(advice_after_tiny, advice_count);

    revoked.store(true, Ordering::SeqCst);
    let revoked_read = route(
        &mut owner,
        "query",
        "engineering.advisory.get",
        json!({
            "task_id":task,"request_key":key
        }),
    )
    .await;
    assert_eq!(revoked_read["opportunity_id"], requested["opportunity_id"]);
    assert!(
        revoked_read.get("current_advice").is_none(),
        "{revoked_read}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let historical: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_matrix_advice WHERE opportunity_id=$1")
            .bind(Uuid::parse_str(requested["opportunity_id"].as_str().unwrap()).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(historical, 1);
    independent.finish().await;
    owner.finish().await;
    server.abort();
}
