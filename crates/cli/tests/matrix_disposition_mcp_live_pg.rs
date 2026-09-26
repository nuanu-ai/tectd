//! Public MCP Matrix disposition proof with a synthetic provider and disposable PG18.
//! The test is ignored until the exact pinned cluster and explicit guard are supplied.
#[path = "pipeline_execution/full_support.rs"]
mod full_support;
#[path = "matrix_disposition_mcp_live_pg/model_route_native.rs"]
mod model_route_native;
#[path = "matrix_disposition_mcp_live_pg/model_route_positive.rs"]
mod model_route_positive;
#[path = "matrix_disposition_mcp_live_pg/pipeline_prepare.rs"]
mod pipeline_prepare;
#[path = "matrix_disposition_mcp_live_pg/planning_effect.rs"]
mod planning_effect;
#[allow(dead_code)]
mod recovery_support;
#[path = "matrix_disposition_mcp_live_pg/selection.rs"]
mod selection;
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
use support::{route, route_error};
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
use tokio::net::UnixListener;
use uuid::Uuid;

const SOCKET: &str = "/tmp/tectd-matrix-pg18.6-fresh-tNXHgM/socket";
const SYSTEM_ID: &str = "7689197957195199396";
const DATABASE_OID: i64 = 16385;
const PROFILE: &str = "synthetic-disposition-provider";
const MODEL: &str = "synthetic-disposition-model";
const RAW: &[u8] = b"synthetic private response bytes";

struct Evidence(Arc<AtomicBool>);

#[async_trait]
impl MatrixEvidenceValidator for Evidence {
    fn policy_version(&self) -> &str {
        "synthetic-disposition-evidence/1"
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
            source: "synthetic-disposition-evidence".into(),
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
        if self.0.load(Ordering::SeqCst)
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

struct Provider(Arc<AtomicUsize>);
impl Provider {
    fn identity_value() -> MatrixProviderIdentity {
        MatrixProviderIdentity {
            provider_profile_ref: AdvisoryProviderProfileRef { id: PROFILE.into() },
            model_configuration: AdvisoryModelConfiguration {
                model: MODEL.into(),
            },
            destination: "https://synthetic.invalid/matrix".into(),
            wire_version: "synthetic-disposition/1".into(),
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
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(MatrixProviderResponse {
            binding: prepared.binding().clone(),
            provider_profile_ref: Self::identity_value().provider_profile_ref,
            model_configuration: Self::identity_value().model_configuration,
            response_payload_sha256: format!("{:x}", Sha256::digest(RAW)),
            raw_response_payload: RAW.to_vec(),
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
    disposable_pair_at_version(58).await
}

async fn disposable_pair_at_version(expected_version: i64) -> (PgPool, String) {
    assert!(matches!(expected_version, 58 | 59));
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
    let admin = PgConnectOptions::from_str(&admin_url).unwrap();
    let runtime = PgConnectOptions::from_str(&runtime_url).unwrap();
    for (options, user) in [(&admin, "postgres"), (&runtime, "tect_ci")] {
        assert_eq!(options.get_username(), user);
        assert_eq!(options.get_database(), Some("tect_test"));
        assert_eq!(options.get_socket().and_then(|p| p.to_str()), Some(SOCKET));
        assert_eq!(options.get_port(), 55479);
    }
    let pool = PgPool::connect_with(admin).await.unwrap();
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
            expected_version
        )
    );
    let runtime_pool = PgPool::connect_with(runtime).await.unwrap();
    let runtime_identity: (String, String, i64) = sqlx::query_as(
        "SELECT current_database(),current_user,(SELECT oid::bigint FROM pg_database WHERE datname=current_database())"
    ).fetch_one(&runtime_pool).await.unwrap();
    assert_eq!(
        runtime_identity,
        ("tect_test".into(), "tect_ci".into(), DATABASE_OID)
    );
    (pool, runtime_url)
}

fn choice(task: Uuid, ids: &[&str]) -> Value {
    json!({"schema":"tect.matrix-choice-set/1","choice_set_id":format!("synthetic-{task}"),
        "version":1,"task_id":task,"task_revision":"1",
        "decision_question":"Which synthetic approach?",
        "candidates":ids.iter().map(|id| json!({"candidate_id":id,
            "title":format!("Approach {id}"),"approach":format!("Synthetic approach {id}"),
            "assumption_fact_ids":[]})).collect::<Vec<_>>()})
}

async fn record_task(owner: &mut Mcp, task: Uuid, ids: &[&str]) -> Value {
    route(
        owner,
        "command",
        "task.source.record",
        json!({
            "task_id":task,"revision":1,"expected_current_revision":0,
            "request_id":Uuid::new_v4(),"input":input(),"choice_set":choice(task, ids)
        }),
    )
    .await
}

async fn verify(independent: &mut Mcp, recorded: &Value, task: Uuid) {
    let parsed: EngineeringMatrixInput = serde_json::from_value(recorded["input"].clone()).unwrap();
    let evidence: Vec<_> = required_matrix_facts(&parsed)
        .unwrap()
        .iter()
        .map(|fact| {
            json!({
                "fact_path":fact.path,"evidence_ref":format!("urn:synthetic:{task}:{}", fact.path)
            })
        })
        .collect();
    let verified = route(
        independent,
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
}

fn disposition(
    recorded: &Value,
    task: Uuid,
    opportunity: &Value,
    basis: &str,
    advice: Option<&Value>,
    decision: Value,
) -> Value {
    json!({"request_id":Uuid::new_v4(),"task_id":task,
        "expected_task_revision":1,"expected_input_digest":recorded["input_digest"],
        "expected_choice_set_digest":recorded["choice_set_digest"],
        "opportunity_id":opportunity["opportunity_id"],"basis":basis,
        "advice_id":advice.map(|a| a["advice_id"].clone()),
        "advice_digest":advice.map(|a| a["advice_digest"].clone()),
        "decision":decision})
}

fn assert_error(result: &Value, allowed: &[&str]) {
    let code = result["error"]["code"].as_str().unwrap();
    assert!(allowed.contains(&code), "unexpected {result}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "writes only the pinned disposable PostgreSQL 18.6 fixture"]
async fn public_mcp_matrix_disposition_is_explicit_guarded_and_readable() {
    let (pool, runtime_url) = disposable_pair().await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("matrix-disposition.sock");
    let revoked = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap()),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(Evidence(revoked.clone())))
        .with_matrix_advisory_adapters(Arc::new(Provider(calls.clone())), Arc::new(Budget)),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));

    let enrolled = admin::enroll_host(&pool, None, vec![]).await.unwrap();
    let owner_config = root.join("owner.json");
    host_file(&owner_config, &enrolled.auth);
    let workspace_key = format!("mcp-matrix-disposition-{}", Uuid::new_v4());
    let mut owner = Mcp::start(
        &socket,
        &owner_config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
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
    route(
        &mut owner,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":0,"mode":"optional","provider_profile_ref":{"id":PROFILE},
            "model_configuration":{"model":MODEL}
        }),
    )
    .await;

    // Ranked advice offers a recommendation, while an independent agent choice picks b.
    let ranked_task = Uuid::new_v4();
    let ranked_source = record_task(&mut owner, ranked_task, &["a", "b"]).await;
    verify(&mut independent, &ranked_source, ranked_task).await;
    let ranked_key = format!("ranked-{}", Uuid::new_v4());
    let ranked = route(
        &mut owner,
        "command",
        "engineering.advisory.request",
        json!({
            "task_id":ranked_task,"expected_task_revision":1,"request_key":ranked_key
        }),
    )
    .await;
    assert_eq!(ranked["state"], "advised", "{ranked}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let current = route(
        &mut owner,
        "query",
        "engineering.advisory.get",
        json!({
            "task_id":ranked_task,"request_key":ranked_key
        }),
    )
    .await;
    assert_eq!(current["current_advice"]["outcome"]["status"], "ranked");
    assert_eq!(
        current["current_advice"]["outcome"]["ranked_choice_ids"],
        json!(["a", "b"])
    );
    let before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_matrix_disposition WHERE opportunity_id=$1",
    )
    .bind(Uuid::parse_str(ranked["opportunity_id"].as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(before, 0, "advice must not choose automatically");
    let selected = disposition(
        &ranked_source,
        ranked_task,
        &ranked,
        "after_advice",
        Some(&current["current_advice"]),
        json!({"outcome":"selected","selected_choice_id":"b"}),
    );
    let receipt = route(
        &mut owner,
        "command",
        "engineering.matrix.disposition.record",
        selected.clone(),
    )
    .await;
    assert_eq!(receipt["decision"], selected["decision"]);
    assert_eq!(receipt["basis"], "after_advice");
    assert_eq!(receipt["advice_id"], current["current_advice"]["advice_id"]);
    let fetched = route(
        &mut owner,
        "query",
        "engineering.matrix.disposition.get",
        json!({
            "task_id":ranked_task,"request_id":selected["request_id"]
        }),
    )
    .await;
    assert_eq!(fetched["disposition_id"], receipt["disposition_id"]);
    assert_eq!(fetched["material_digest"], receipt["material_digest"]);
    let replay = route(
        &mut owner,
        "command",
        "engineering.matrix.disposition.record",
        selected.clone(),
    )
    .await;
    assert_eq!(replay["disposition_id"], receipt["disposition_id"]);
    let mut conflict = selected.clone();
    conflict["decision"] = json!({"outcome":"blocked","blocked_reason":"Changed decision"});
    assert_error(
        &route_error(
            &mut owner,
            "command",
            "engineering.matrix.disposition.record",
            conflict,
        )
        .await,
        &["input_conflict"],
    );
    let mut wrong_choice = selected.clone();
    wrong_choice["request_id"] = json!(Uuid::new_v4());
    wrong_choice["decision"] = json!({"outcome":"selected","selected_choice_id":"outside"});
    assert_error(
        &route_error(
            &mut owner,
            "command",
            "engineering.matrix.disposition.record",
            wrong_choice,
        )
        .await,
        &["invalid_arguments"],
    );
    let mut stale_advice = selected.clone();
    stale_advice["request_id"] = json!(Uuid::new_v4());
    stale_advice["advice_digest"] = json!("0".repeat(64));
    assert_error(
        &route_error(
            &mut owner,
            "command",
            "engineering.matrix.disposition.record",
            stale_advice,
        )
        .await,
        &["stale_context"],
    );
    let mut stale_rev = selected.clone();
    stale_rev["request_id"] = json!(Uuid::new_v4());
    stale_rev["expected_task_revision"] = json!(2);
    assert_error(
        &route_error(
            &mut owner,
            "command",
            "engineering.matrix.disposition.record",
            stale_rev,
        )
        .await,
        &["stale_revision"],
    );
    let mut other_session = Mcp::start(
        &socket,
        &owner_config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    other_session.call("open_workspace", json!({})).await;
    assert_error(
        &route_error(
            &mut other_session,
            "command",
            "engineering.matrix.disposition.record",
            selected.clone(),
        )
        .await,
        &["input_conflict"],
    );
    assert_error(
        &route_error(
            &mut independent,
            "command",
            "engineering.matrix.disposition.record",
            selected.clone(),
        )
        .await,
        &["input_conflict", "forbidden"],
    );
    let other_get = route_error(
        &mut other_session,
        "query",
        "engineering.matrix.disposition.get",
        json!({
            "task_id":ranked_task,"request_id":selected["request_id"]
        }),
    )
    .await;
    assert_error(&other_get, &["not_found"]);

    // A verified singleton is ineligible for JEV, but remains selectable by the agent.
    let single_task = Uuid::new_v4();
    let single_source = record_task(&mut owner, single_task, &["one"]).await;
    verify(&mut independent, &single_source, single_task).await;
    let singleton = route(
        &mut owner,
        "command",
        "engineering.advisory.request",
        json!({
            "task_id":single_task,"expected_task_revision":1,
            "request_key":format!("singleton-{}", Uuid::new_v4())
        }),
    )
    .await;
    assert_eq!(singleton["state"], "no_call");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let single_selected = disposition(
        &single_source,
        single_task,
        &singleton,
        "no_call",
        None,
        json!({"outcome":"selected","selected_choice_id":"one"}),
    );
    let single_receipt = route(
        &mut owner,
        "command",
        "engineering.matrix.disposition.record",
        single_selected.clone(),
    )
    .await;
    assert_eq!(single_receipt["decision"], single_selected["decision"]);
    let single_get = route(
        &mut owner,
        "query",
        "engineering.matrix.disposition.get",
        json!({
            "task_id":single_task,"request_id":single_selected["request_id"]
        }),
    )
    .await;
    assert_eq!(
        single_get["disposition_id"],
        single_receipt["disposition_id"]
    );

    // A historical source-unverified no-call is auditable, but cannot select.
    let unverified_task = Uuid::new_v4();
    let unverified_source = record_task(&mut owner, unverified_task, &["a", "b"]).await;
    let unverified = route(
        &mut owner,
        "command",
        "engineering.advisory.request",
        json!({
            "task_id":unverified_task,"expected_task_revision":1,
            "request_key":format!("unverified-{}", Uuid::new_v4())
        }),
    )
    .await;
    assert_eq!(unverified["state"], "no_call");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let unsupported_select = disposition(
        &unverified_source,
        unverified_task,
        &unverified,
        "no_call",
        None,
        json!({"outcome":"selected","selected_choice_id":"a"}),
    );
    assert_error(
        &route_error(
            &mut owner,
            "command",
            "engineering.matrix.disposition.record",
            unsupported_select,
        )
        .await,
        &["stale_context"],
    );
    let blocked = disposition(
        &unverified_source,
        unverified_task,
        &unverified,
        "no_call",
        None,
        json!({"outcome":"blocked","blocked_reason":"Independent source verification absent"}),
    );
    let blocked_receipt = route(
        &mut owner,
        "command",
        "engineering.matrix.disposition.record",
        blocked.clone(),
    )
    .await;
    assert_eq!(blocked_receipt["decision"], blocked["decision"]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Evidence loss rejects a fresh selected disposition while the old row stays readable.
    revoked.store(true, Ordering::SeqCst);
    let mut stale_evidence = selected.clone();
    stale_evidence["request_id"] = json!(Uuid::new_v4());
    assert_error(
        &route_error(
            &mut owner,
            "command",
            "engineering.matrix.disposition.record",
            stale_evidence,
        )
        .await,
        &["stale_context"],
    );
    let retained = route(
        &mut owner,
        "query",
        "engineering.matrix.disposition.get",
        json!({
            "task_id":ranked_task,"request_id":selected["request_id"]
        }),
    )
    .await;
    assert_eq!(retained["disposition_id"], receipt["disposition_id"]);
    for payload in [&current, &receipt, &fetched, &blocked_receipt, &retained] {
        let text = payload.to_string();
        assert!(!text.contains("synthetic private response bytes"), "{text}");
        assert!(!text.contains("raw_response_payload"), "{text}");
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_matrix_disposition WHERE opportunity_id=$1",
    )
    .bind(Uuid::parse_str(ranked["opportunity_id"].as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    other_session.finish().await;
    independent.finish().await;
    owner.finish().await;
    server.abort();
}
