//! Public MCP Matrix verification with a deterministic host-owned evidence source.
//! This ignored test writes only to the exact disposable PG18.6 fixture cluster.
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
mod support;

use async_trait::async_trait;
use recovery_support::{Mcp, host_file, private_temp};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgConnectOptions};
use std::{
    os::unix::fs::PermissionsExt,
    str::FromStr,
    sync::{Arc, Mutex},
};
use support::{route, route_error};
use tect_application::{MatrixEvidenceValidator, WorkspaceService};
use tect_domain::{
    EngineeringMatrixInput, EvidenceValidationOutcome, MatrixEvidenceBinding, RequestContext,
    RequiredMatrixFact, Result, required_matrix_facts,
};
use tect_postgres::{PgStore, admin};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};
use uuid::Uuid;

const SYSTEM_ID: &str = "7689109430044371904";
const DATABASE_OID: i64 = 16384;

struct DeterministicEvidence {
    accepted: Mutex<Vec<MatrixEvidenceBinding>>,
}

#[derive(sqlx::FromRow)]
struct StoredFact {
    fact_path: String,
    value_digest: String,
    content_digest: String,
    evidence_ref: String,
    source: String,
    subject: String,
    observed_at: i64,
    expires_at: i64,
    validation_outcome: String,
}

#[async_trait]
impl MatrixEvidenceValidator for DeterministicEvidence {
    fn policy_version(&self) -> &str {
        "mcp-fixture-policy/1"
    }

    async fn validate(
        &self,
        _: Uuid,
        task_id: Uuid,
        _: i64,
        fact: &RequiredMatrixFact,
        evidence_ref: &str,
        now: i64,
    ) -> Result<MatrixEvidenceBinding> {
        let binding = MatrixEvidenceBinding {
            fact_path: fact.path.clone(),
            value_digest: fact.value_digest.clone(),
            evidence_ref: evidence_ref.into(),
            content_digest: format!("{:x}", Sha256::digest(evidence_ref.as_bytes())),
            source: "synthetic-mcp-fixture".into(),
            subject: task_id.to_string(),
            observed_at: now - 10,
            expires_at: now + 3600,
            validation_outcome: EvidenceValidationOutcome::Accepted,
        };
        self.accepted.lock().unwrap().push(binding.clone());
        Ok(binding)
    }
}

fn input() -> Value {
    json!({
        "mode":{"state":"known","value":"mvp","provenance":"synthetic owner source"},
        "envelope":{
            "scale":{"state":"known","value":"12 workers","provenance":"synthetic owner source"},
            "operational_facts":{"state":"known_empty","provenance":"synthetic owner source"}
        },
        "criticality":{"state":"known","value":"low","provenance":"synthetic owner source"},
        "intent":{"state":"known","value":{"kind":"other","description":"booking"},"provenance":"synthetic owner source"},
        "urgency":{"state":"known","value":"normal","provenance":"synthetic owner source"},
        "promised_behavior":{"state":"known","value":"books","provenance":"synthetic owner source"},
        "promised_proof":{"state":"known","value":"acceptance","provenance":"synthetic owner source"},
        "affected_guarantees":{"state":"known_empty","provenance":"synthetic owner source"},
        "actual_exposure":{"state":"known","value":false,"provenance":"synthetic owner source"},
        "demand_commitment":{"state":"known","value":"no_commitment","provenance":"synthetic owner source"},
        "latency_commitment":{"state":"known","value":"no_commitment","provenance":"synthetic owner source"},
        "urgent_repair":{"state":"known","value":false,"provenance":"synthetic owner source"}
    })
}

async fn disposable_pair() -> (PgPool, String, String) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    assert_eq!(role, "tect_ci");
    let admin = PgConnectOptions::from_str(&admin_url).unwrap();
    let runtime = PgConnectOptions::from_str(&runtime_url).unwrap();
    assert_eq!(admin.get_username(), "postgres");
    assert_eq!(runtime.get_username(), role);
    for options in [&admin, &runtime] {
        assert_eq!(options.get_database(), Some("tect_test"));
        assert_eq!(
            options.get_socket().and_then(|p| p.to_str()),
            Some("/tmp/tectd-matrix-pg18.6-Ry8kWr/socket")
        );
        assert_eq!(options.get_port(), 55586);
    }
    let admin_pool = PgPool::connect_with(admin).await.unwrap();
    let runtime_pool = PgPool::connect_with(runtime).await.unwrap();
    let identity: (i32, String, String, i64, String) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),current_user,\
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_control_system())",
    )
    .fetch_one(&admin_pool)
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
    let runtime_identity: (String, String, i64) = sqlx::query_as(
        "SELECT current_database(),current_user,(SELECT oid::bigint FROM pg_database WHERE datname=current_database())")
        .fetch_one(&runtime_pool).await.unwrap();
    assert_eq!(
        runtime_identity,
        ("tect_test".into(), role.clone(), DATABASE_OID)
    );
    (admin_pool, runtime_url, role)
}

async fn verification_count(pool: &PgPool, task_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM matrix_verifications WHERE task_id=$1")
        .bind(task_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// The bounded host wire is beneath MCP; MCP itself always requests the default capacity.
async fn bounded_host_call(
    socket: &std::path::Path,
    context: &RequestContext,
    params: Value,
) -> Value {
    let mut stream = UnixStream::connect(socket).await.unwrap();
    let request = json!({"api_version":2,"context":context,"tool_name":"verify_matrix_task",
        "arguments":params,"output_capacity":1});
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
#[ignore = "writes exact disposable PostgreSQL 18.6 fixture with TECT_TEST_DISPOSABLE_PG=1"]
async fn public_mcp_matrix_verify_seals_independent_receipt() {
    let (pool, runtime_url, role) = disposable_pair().await;
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("matrix-verify.sock");
    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
    let validator = Arc::new(DeterministicEvidence {
        accepted: Mutex::new(Vec::new()),
    });
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(store),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(validator.clone()),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));

    let enrolled = admin::enroll_host(&pool, None, vec![]).await.unwrap();
    let owner_config = root.join("owner.json");
    host_file(&owner_config, &enrolled.auth);
    let workspace_key = format!("mcp-matrix-{}", Uuid::new_v4());
    let owner_native = Uuid::new_v4().to_string();
    let mut owner = Mcp::start(&socket, &owner_config, &owner_native, &workspace_key).await;
    let tools = owner.exchange("tools/list", json!({})).await;
    let names: Vec<_> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["get_state", "query", "command", "execute", "help"]);
    let opened = owner.call("open_workspace", json!({})).await;
    let workspace_id = Uuid::parse_str(opened["workspace"]["id"].as_str().unwrap()).unwrap();
    let task_id = Uuid::new_v4();
    let recorded = route(
        &mut owner,
        "command",
        "task.source.record",
        json!({
            "task_id":task_id,"revision":1,"expected_current_revision":0,
            "request_id":Uuid::new_v4(),"input":input()
        }),
    )
    .await;
    let digest = recorded["input_digest"].as_str().unwrap().to_owned();
    let revision = recorded["revision"].as_i64().unwrap();
    assert_eq!(revision, 1);
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
    let params = json!({"task_id":task_id,"expected_revision":revision,
        "input_digest":digest,"evidence":evidence});
    assert_eq!(verification_count(&pool, task_id).await, 0);
    let owner_denial = route_error(
        &mut owner,
        "command",
        "engineering.matrix.verify",
        params.clone(),
    )
    .await;
    assert_eq!(owner_denial["error"]["code"], "forbidden");

    let verifier = admin::prepare_verifier_enrollment(&pool, enrolled.tenant_id, workspace_id)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let verifier_config = root.join("verifier.json");
    host_file(&verifier_config, &verifier.auth);
    let verifier_native = Uuid::new_v4().to_string();
    let mut independent =
        Mcp::start(&socket, &verifier_config, &verifier_native, &workspace_key).await;
    let verifier_opened = independent.call("open_workspace", json!({})).await;
    let verifier_session_id =
        Uuid::parse_str(verifier_opened["session"]["id"].as_str().unwrap()).unwrap();
    let mut malformed = params.clone();
    malformed["evidence"][0]["validation_outcome"] = json!("accepted");
    let invalid = route_error(
        &mut independent,
        "command",
        "engineering.matrix.verify",
        malformed.clone(),
    )
    .await;
    assert_eq!(invalid["error"]["code"], "invalid_arguments");
    let forbidden = route_error(
        &mut owner,
        "command",
        "engineering.matrix.verify",
        malformed,
    )
    .await;
    assert_eq!(forbidden["error"]["code"], "forbidden");
    assert_eq!(verification_count(&pool, task_id).await, 0);
    let context = RequestContext {
        auth: verifier.auth.clone(),
        native_session_id: verifier_native,
        workspace_key: workspace_key.clone(),
    };
    let tiny = bounded_host_call(&socket, &context, params.clone()).await;
    assert_eq!(tiny["error"], "request_too_large");
    assert_eq!(verification_count(&pool, task_id).await, 0);

    let receipt = route(
        &mut independent,
        "command",
        "engineering.matrix.verify",
        params,
    )
    .await;
    assert_eq!(receipt["task_id"], task_id.to_string());
    assert_eq!(receipt["task_revision"], "1");
    assert_eq!(receipt["input_digest"], digest);
    assert_eq!(receipt["policy_version"], "mcp-fixture-policy/1");
    let sealed = receipt["verification_digest"].as_str().unwrap();
    assert_eq!(sealed.len(), 64);
    assert_eq!(receipt["facts"].as_array().unwrap().len(), evidence.len());
    assert!(
        receipt["facts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|fact| fact["status"] == "accepted")
    );
    let row: (String, String, Uuid, Uuid, Uuid) = sqlx::query_as(
        "SELECT record_digest,input_digest,owner_principal_id,verifier_principal_id,verifier_session_id FROM matrix_verifications WHERE task_id=$1")
        .bind(task_id).fetch_one(&pool).await.unwrap();
    assert_eq!((row.0.as_str(), row.1.as_str()), (sealed, digest.as_str()));
    assert_eq!(
        (row.2, row.3),
        (enrolled.principal_id, verifier.principal_id)
    );
    assert_eq!(row.4, verifier_session_id);
    assert_eq!(verification_count(&pool, task_id).await, 1);
    let stored_facts: Vec<StoredFact> = sqlx::query_as(
        "SELECT fact_path,value_digest,content_digest,evidence_ref,source,subject,\
         observed_at,expires_at,validation_outcome FROM matrix_verification_bindings \
         WHERE verification_id=(SELECT id FROM matrix_verifications WHERE task_id=$1) \
         ORDER BY fact_path",
    )
    .bind(task_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(stored_facts.len(), evidence.len());
    let accepted = validator.accepted.lock().unwrap().clone();
    assert_eq!(accepted.len(), evidence.len());
    for ((saved, returned), expected) in stored_facts
        .iter()
        .zip(receipt["facts"].as_array().unwrap())
        .zip(accepted.iter())
    {
        assert_eq!(returned["fact_path"], saved.fact_path);
        assert_eq!(returned["value_digest"], saved.value_digest);
        assert_eq!(returned["content_digest"], saved.content_digest);
        assert_eq!(saved.fact_path, expected.fact_path);
        assert_eq!(saved.value_digest, expected.value_digest);
        assert_eq!(saved.content_digest, expected.content_digest);
        assert_eq!(saved.evidence_ref, expected.evidence_ref);
        assert_eq!(saved.source, expected.source);
        assert_eq!(saved.subject, expected.subject);
        assert_eq!(saved.observed_at, expected.observed_at);
        assert_eq!(saved.expires_at, expected.expires_at);
        assert_eq!(saved.validation_outcome, "accepted");
        assert_eq!(
            expected.validation_outcome,
            EvidenceValidationOutcome::Accepted
        );
    }
    let readback = route(
        &mut independent,
        "query",
        "task.source.get",
        json!({"task_id":task_id}),
    )
    .await;
    assert_eq!(readback["input_digest"], digest);
    assert_eq!(readback["revision"], revision);
    owner.finish().await;
    independent.finish().await;
    server.abort();
}
