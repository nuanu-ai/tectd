//! S02 public native MCP proof on synthetic declarations and evidence.
//! The owner response reference is fixture data, never a claim of human consent.
//! Ignored: writes only after the exact disposable PostgreSQL 18.6 identity guard.
#[allow(dead_code)]
mod recovery_support;
#[path = "matrix_context_advisory_native_mcp/s03_v4.rs"]
mod s03_v4;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use async_trait::async_trait;
use recovery_support::{Mcp, host_file, private_temp};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};
use sha2::{Digest, Sha256, Sha384};
use sqlx::{PgPool, postgres::PgConnectOptions};
use std::{
    os::unix::fs::PermissionsExt,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use support::{id, ready_source_candidate, repository, route, route_error};
use tect_application::{
    MatrixAdviceProvider, MatrixBudgetAuthorization, MatrixBudgetPolicy, MatrixBudgetRequest,
    MatrixEvidenceValidator, MatrixProviderIdentity, MatrixProviderRequest, MatrixProviderResponse,
    MatrixStartedDispatchPermit, MatrixVerificationAuthority, PreparedMatrixAdviceAttempt, Store,
    TransactionMode, WorkspaceService,
};
use tect_domain::{
    AdvisoryBudgetCeilings, AdvisoryBudgetPolicy, AdvisoryModelConfiguration,
    AdvisoryProviderProfileRef, EffectiveMatrixRequirements, EngineeringMatrixInput, EventKind,
    EvidenceValidationOutcome, MatrixEvidenceBinding, MatrixRanking, RequiredMatrixFact, Result,
    required_matrix_operating_facts,
};
use tect_postgres::{BudgetOwnerKeys, PgStore, admin};
use tokio::net::UnixListener;
use uuid::Uuid;

const SYSTEM_ID: &str = "7689676854994613066";
const DATABASE_OID: i64 = 16385;
const OWNER_RESPONSE: &str = "fixture:synthetic-owner-response-not-human-consent";
const PROFILE: &str = "synthetic-context-provider";
const MODEL: &str = "synthetic-context-model";

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
        assert_eq!(options.get_host(), "127.0.0.1");
        assert_eq!(options.get_port(), 64775);
        assert!(options.get_socket().is_none());
    }
    let pool = PgPool::connect_with(admin_options).await.unwrap();
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
    let runtime = PgPool::connect_with(runtime_options).await.unwrap();
    let runtime_identity: (String, String, i64) = sqlx::query_as(
        "SELECT current_database(),current_user,(SELECT oid::bigint FROM pg_database WHERE datname=current_database())",
    ).fetch_one(&runtime).await.unwrap();
    assert_eq!(
        runtime_identity,
        ("tect_test".into(), "tect_ci".into(), DATABASE_OID)
    );
    let ledger: Vec<(i64, bool, Vec<u8>)> =
        sqlx::query_as("SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        ledger.len(),
        110,
        "fixture must have exactly migrations 1 through 110"
    );
    for (index, (version, success, _)) in ledger.iter().enumerate() {
        assert_eq!(*version, index as i64 + 1);
        assert!(*success, "migration {version} is not successful");
    }
    for (version, bytes) in [
        (
            105,
            include_bytes!(
                "../../postgres/migrations/0105_matrix_declared_requirements_context.sql"
            )
            .as_slice(),
        ),
        (
            106,
            include_bytes!("../../postgres/migrations/0106_matrix_task_requirements_binding.sql")
                .as_slice(),
        ),
        (
            107,
            include_bytes!("../../postgres/migrations/0107_context_matrix_verification.sql")
                .as_slice(),
        ),
        (
            108,
            include_bytes!(
                "../../postgres/migrations/0108_matrix_v1_dispatch_cutover_allowlist.sql"
            )
            .as_slice(),
        ),
        (
            109,
            include_bytes!("../../postgres/migrations/0109_matrix_planning_context_selection.sql")
                .as_slice(),
        ),
        (
            110,
            include_bytes!("../../postgres/migrations/0110_pipeline_context_matrix_authority.sql")
                .as_slice(),
        ),
    ] {
        assert_eq!(
            ledger[(version - 1) as usize].2,
            Sha384::digest(bytes).to_vec(),
            "fixture migration {version} differs from reviewed source"
        );
    }
    (pool, runtime_url)
}

struct Evidence;
#[async_trait]
impl MatrixEvidenceValidator for Evidence {
    fn policy_version(&self) -> &str {
        "synthetic-context-policy/1"
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
            source: "synthetic-operating-evidence".into(),
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
        if binding.fact_path != fact.path
            || binding.value_digest != fact.value_digest
            || binding.expires_at <= now
        {
            return Err(tect_domain::Error::Forbidden);
        }
        Ok(())
    }
}
struct Budget(Arc<AtomicUsize>);
#[async_trait]
impl MatrixBudgetPolicy for Budget {
    async fn authorize(
        &self,
        _: &MatrixBudgetRequest,
        policy: &tect_domain::AdvisoryBudgetPolicy,
    ) -> Result<Option<MatrixBudgetAuthorization>> {
        self.0.fetch_add(1, Ordering::SeqCst);
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
            destination: "https://synthetic.invalid/context".into(),
            wire_version: "synthetic-context/1".into(),
        }
    }
}
#[async_trait]
impl MatrixAdviceProvider for Provider {
    fn identity(&self) -> Option<MatrixProviderIdentity> {
        Some(Self::identity_value())
    }
    fn prepare(&self, request: &MatrixProviderRequest) -> Result<PreparedMatrixAdviceAttempt> {
        let binding = request.binding();
        let MatrixVerificationAuthority::ContextV2 {
            digest,
            snapshot_id,
            authority_schema,
            semantic_digest,
        } = &binding.verification
        else {
            return Err(tect_domain::Error::Forbidden);
        };
        let choice = request
            .revision()
            .choice_set
            .as_ref()
            .ok_or(tect_domain::Error::InvalidArguments)?;
        let candidate_ids: Vec<_> = choice
            .candidates
            .iter()
            .map(|candidate| candidate.candidate_id.as_str())
            .collect();
        let body = serde_json::to_vec(&json!({
            "model":MODEL,
            "state":{
                "contract":"tect.context-matrix-verified-evaluation/1",
                "binding":{
                    "task_id":binding.task_id.to_string(),
                    "task_revision":binding.task_revision.to_string(),
                    "input_digest":binding.input_digest,
                    "choice_set_id":binding.choice_set_id,
                    "choice_set_version":binding.choice_set_version,
                    "choice_set_digest":binding.choice_set_digest,
                    "verification_digest":digest,
                    "evaluation_digest":binding.evaluation_digest,
                    "context":{
                        "schema":"tect.context-matrix-verification/1",
                        "frozen_snapshot_id":snapshot_id,
                        "authority_schema":authority_schema,
                        "requirements_semantic_digest":semantic_digest,
                    }
                },
                "input":request.revision().input,
                "composition":request.composition(),
                "choice_set":choice,
            },
            "questions":{"ranking":{
                "type":"ranking",
                "instructions":"Rank synthetic candidates from fixture evidence only; this advice authorizes no action.",
                "candidate_ids":candidate_ids,
            }}
        }))
        .map_err(|_| tect_domain::Error::InvalidArguments)?;
        PreparedMatrixAdviceAttempt::new(request, Self::identity_value(), body)
    }
    async fn attempt_prepared(
        &self,
        prepared: PreparedMatrixAdviceAttempt,
        _: MatrixStartedDispatchPermit,
    ) -> Result<MatrixProviderResponse> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let raw = b"synthetic private response".to_vec();
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

fn locator(program: Uuid) -> Value {
    json!({"level":"program","program_id":program})
}
fn declaration(kind: &str, value: Value) -> Value {
    json!({"operation":"set","value":{"kind":kind,"value":value}})
}
fn declarations(mode: &str) -> Vec<Value> {
    vec![
        declaration("mode", json!(mode)),
        declaration(
            "intent",
            json!({"kind":"other","description":"synthetic build"}),
        ),
        declaration("urgency", json!("ordinary")),
        declaration("promised_behavior", json!("synthetic behavior")),
        declaration("promised_proof", json!("synthetic check")),
        json!({"operation":"set","value":{"kind":"no_demand_commitment"}}),
        json!({"operation":"set","value":{"kind":"no_latency_commitment"}}),
    ]
}
async fn propose_confirm(
    owner: &mut Mcp,
    program: Uuid,
    expected: u64,
    patches: Vec<Value>,
) -> Value {
    let loc = locator(program);
    let proposal = route(owner, "command", "engineering.matrix.context.propose",
        json!({"request_id":Uuid::new_v4(),"locator":loc,"expected_context_revision":expected,"patches":patches})).await;
    route(owner, "command", "engineering.matrix.context.confirm", json!({
        "request_id":Uuid::new_v4(),"locator":locator(program),
        "proposal_revision":proposal["proposal"]["revision"],"proposal_digest":proposal["proposal"]["digest"],
        "owner_response_ref":OWNER_RESPONSE,
    })).await
}
fn input() -> Value {
    json!({
        "mode":{"state":"absent"},"intent":{"state":"absent"},"urgency":{"state":"absent"},
        "promised_behavior":{"state":"absent"},"promised_proof":{"state":"absent"},
        "demand_commitment":{"state":"absent"},"latency_commitment":{"state":"absent"},
        "envelope":{"scale":{"state":"known","value":"one synthetic request","provenance":"fixture observation"},
            "operational_facts":{"state":"known_empty","provenance":"fixture observation"}},
        "criticality":{"state":"known","value":"low","provenance":"fixture observation"},
        "affected_guarantees":{"state":"known_empty","provenance":"fixture observation"},
        "actual_exposure":{"state":"known","value":false,"provenance":"fixture observation"},
        "urgent_repair":{"state":"known","value":false,"provenance":"fixture observation"}
    })
}
fn choice(task: Uuid) -> Value {
    json!({"schema":"tect.matrix-choice-set/1","choice_set_id":"synthetic-context-choice","version":1,
    "task_id":task.to_string(),"task_revision":"1","decision_question":"Which synthetic approach?",
    "candidates":[
        {"candidate_id":"a","title":"Approach a","approach":"Synthetic approach a","assumption_fact_ids":[]},
        {"candidate_id":"b","title":"Approach b","approach":"Synthetic approach b","assumption_fact_ids":[]}
    ]})
}
async fn record(owner: &mut Mcp, task: Uuid, request: Uuid, loc: Option<Value>) -> Value {
    let mut args = json!({"task_id":task,"revision":1,"expected_current_revision":0,
        "request_id":request,"input":input(),"choice_set":choice(task)});
    if let Some(loc) = loc {
        args["requirements_locator"] = loc;
    }
    route(owner, "command", "task.source.record", args).await
}
async fn advice(owner: &mut Mcp, task: Uuid, key: &str) -> Value {
    route(
        owner,
        "command",
        "engineering.advisory.request",
        json!({"task_id":task,"expected_task_revision":1,"request_key":key}),
    )
    .await
}

async fn signed_fixture_budget(
    store: &PgStore,
    enrolled: &tect_postgres::admin::Enrollment,
    workspace_key: &str,
) -> (Uuid, BudgetOwnerKeys) {
    signed_fixture_budget_with_calls(store, enrolled, workspace_key, 8).await
}

async fn signed_fixture_budget_with_calls(
    store: &PgStore,
    enrolled: &tect_postgres::admin::Enrollment,
    workspace_key: &str,
    provider_calls: i64,
) -> (Uuid, BudgetOwnerKeys) {
    signed_fixture_budget_with_token_ceilings(
        store,
        enrolled,
        workspace_key,
        provider_calls,
        1_000,
        1_000,
    )
    .await
}

async fn signed_fixture_budget_with_token_ceilings(
    store: &PgStore,
    enrolled: &tect_postgres::admin::Enrollment,
    workspace_key: &str,
    provider_calls: i64,
    input_tokens: i64,
    output_tokens: i64,
) -> (Uuid, BudgetOwnerKeys) {
    let keypair = Ed25519KeyPair::from_seed_unchecked(&[91_u8; 32]).unwrap();
    let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    tx.authenticate(&enrolled.auth).await.unwrap();
    tx.set_tenant(enrolled.tenant_id).await.unwrap();
    let created = tx.ensure_workspace(workspace_key).await.unwrap();
    let workspace = created.value.id;
    tx.ensure_membership(workspace, enrolled.principal_id)
        .await
        .unwrap();
    if created.created {
        tx.append_creation_event(workspace, EventKind::WorkspaceOpened, workspace)
            .await
            .unwrap();
    }
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
        provider_calls,
        input_tokens,
        output_tokens,
        request_utf8_bytes: 2_000_000,
        elapsed_monotonic_ms: 120_000,
        retry_dispatches: 1,
    };
    let digest = AdvisoryBudgetPolicy::digest_for(id, 1, from, until, ceilings);
    let unsigned = AdvisoryBudgetPolicy::new(
        id,
        1,
        digest.clone(),
        from,
        until,
        ceilings,
        enrolled.principal_id,
        "0".repeat(128),
    )
    .unwrap();
    let signature = keypair.sign(&unsigned.approval_signing_message(workspace).unwrap());
    let hex = |bytes: &[u8]| {
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let policy = AdvisoryBudgetPolicy::new(
        id,
        1,
        digest,
        from,
        until,
        ceilings,
        enrolled.principal_id,
        hex(signature.as_ref()),
    )
    .unwrap();
    tx.advisory_budget_policy_store()
        .unwrap()
        .install_budget_policy(workspace, &policy)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let keys = BudgetOwnerKeys::from_json(
        &json!([{
            "workspace_id":workspace,"owner_id":enrolled.principal_id,
            "public_key_hex":hex(keypair.public_key().as_ref()),
        }])
        .to_string(),
    )
    .unwrap();
    (workspace, keys)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "writes only exact owned disposable PostgreSQL 18.6 fixture"]
async fn public_context_v2_binding_verification_and_advisory_currentness() {
    let (pool, runtime_url) = disposable_pair().await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("matrix-context-advisory.sock");
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let workspace_key = format!("matrix-context-advice-{}", Uuid::new_v4());
    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
    let (workspace, owner_keys) = signed_fixture_budget(&store, &enrolled, &workspace_key).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let budgets = Arc::new(AtomicUsize::new(0));
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(store.with_budget_owner_keys(owner_keys)),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(Evidence))
        .with_matrix_advisory_adapters(
            Arc::new(Provider(calls.clone())),
            Arc::new(Budget(budgets.clone())),
        ),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    let owner_host = root.join("owner.json");
    host_file(&owner_host, &enrolled.auth);
    let mut owner = Mcp::start(
        &socket,
        &owner_host,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    let (source, _) = ready_source_candidate(&mut owner, &repo).await;
    assert_eq!(
        id(&owner.call("open_workspace", json!({})).await["workspace"]["id"]),
        workspace
    );
    let program: Uuid =
        sqlx::query_scalar("SELECT program_id FROM scope_candidate_sets WHERE id=$1")
            .bind(id(&source["candidate_set"]["id"]))
            .fetch_one(&pool)
            .await
            .unwrap();
    let loc = locator(program);
    let confirmed = propose_confirm(&mut owner, program, 0, declarations("demo")).await;
    assert_eq!(confirmed["confirmation"]["proposal_revision"], 1);
    let effective = route(
        &mut owner,
        "query",
        "engineering.matrix.context.effective.get",
        json!({"locator":loc}),
    )
    .await;
    let original_semantic = effective["semantic_digest"].clone();
    assert_eq!(
        effective["values"]["mode"]["value"],
        json!({"kind":"mode","value":"demo"})
    );

    let task = Uuid::new_v4();
    let request = Uuid::new_v4();
    let recorded = record(&mut owner, task, request, Some(locator(program))).await;
    assert_eq!(recorded["input"]["mode"]["value"], "demo");
    assert_eq!(
        recorded["input"]["intent"]["value"]["description"],
        "synthetic build"
    );
    let snapshot = recorded["requirements_snapshot_id"].clone();
    assert_eq!(recorded["requirements_semantic_digest"], original_semantic);
    assert_eq!(
        recorded["context_authority_schema"],
        "tect.matrix-requirements/1"
    );
    let replay = record(&mut owner, task, request, Some(locator(program))).await;
    assert_eq!(replay["requirements_snapshot_id"], snapshot);
    assert_eq!(replay["requirements_semantic_digest"], original_semantic);
    assert_eq!(replay["input_digest"], recorded["input_digest"]);
    let fetched = route(
        &mut owner,
        "query",
        "task.source.get",
        json!({"task_id":task}),
    )
    .await;
    assert_eq!(fetched["requirements_snapshot_id"], snapshot);
    assert_eq!(fetched["requirements_semantic_digest"], original_semantic);
    assert_eq!(fetched["input_digest"], recorded["input_digest"]);
    let stored_binding: (Uuid, String) = sqlx::query_as(
        "SELECT snapshot_id,semantic_digest FROM matrix_task_requirements_bindings WHERE task_id=$1 AND revision=1")
        .bind(task).fetch_one(&pool).await.unwrap();
    assert_eq!(
        stored_binding,
        (
            id(&snapshot),
            original_semantic.as_str().unwrap().to_owned()
        )
    );

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
    let verifier = admin::prepare_verifier_enrollment(&pool, enrolled.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let verifier_host = root.join("verifier.json");
    host_file(&verifier_host, &verifier.auth);
    let mut independent = Mcp::start(
        &socket,
        &verifier_host,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    independent.call("open_workspace", json!({})).await;
    let parsed: EngineeringMatrixInput = serde_json::from_value(recorded["input"].clone()).unwrap();
    let context: EffectiveMatrixRequirements = serde_json::from_value(effective).unwrap();
    let operating = required_matrix_operating_facts(&context, &parsed).unwrap();
    assert!(!operating.is_empty());
    assert!(operating.iter().all(|fact| {
        ![
            "/mode",
            "/intent",
            "/urgency",
            "/promised_behavior",
            "/promised_proof",
            "/demand_commitment",
            "/latency_commitment",
        ]
        .contains(&fact.path.as_str())
    }));
    let missing = route_error(&mut independent, "command", "engineering.matrix.verify", json!({
        "task_id":task,"expected_revision":1,"input_digest":recorded["input_digest"],"evidence":[]
    })).await;
    assert_eq!(missing["error"]["code"], "invalid_arguments");
    let evidence: Vec<_> = operating
        .iter()
        .map(|fact| {
            json!({
                "fact_path":fact.path,"evidence_ref":format!("urn:fixture:operating:{}", fact.path)
            })
        })
        .collect();
    let verified = route(&mut independent, "command", "engineering.matrix.verify", json!({
        "task_id":task,"expected_revision":1,"input_digest":recorded["input_digest"],"evidence":evidence
    })).await;
    assert_eq!(verified["schema"], "tect.context-matrix-verification/1");
    assert_eq!(verified["frozen_snapshot_id"], snapshot);
    assert_eq!(verified["facts"].as_array().unwrap().len(), operating.len());
    let saved_verification: (Uuid, String, String) = sqlx::query_as(
        "SELECT frozen_snapshot_id,requirements_semantic_digest,record_digest FROM matrix_verifications WHERE task_id=$1 AND task_revision=1")
        .bind(task).fetch_one(&pool).await.unwrap();
    assert_eq!(saved_verification.0, id(&snapshot));
    assert_eq!(saved_verification.1, original_semantic.as_str().unwrap());
    assert_eq!(
        saved_verification.2,
        verified["verification_digest"].as_str().unwrap()
    );

    // A second confirmation changes provenance/revision only. Dispatch must still bind the original frozen snapshot.
    propose_confirm(&mut owner, program, 1, declarations("demo")).await;
    let refreshed = route(
        &mut owner,
        "query",
        "engineering.matrix.context.effective.get",
        json!({"locator":locator(program)}),
    )
    .await;
    assert_eq!(refreshed["semantic_digest"], original_semantic);
    assert_ne!(
        refreshed["values"]["mode"]["source"],
        context
            .values()
            .get(&tect_domain::DeclaredRequirementPath::Mode)
            .map(|v| serde_json::to_value(&v.source).unwrap())
            .unwrap()
    );
    let requested = advice(
        &mut owner,
        task,
        &format!("same-semantic-{}", Uuid::new_v4()),
    )
    .await;
    assert_eq!(requested["state"], "advised", "{requested}");
    assert_eq!(requested["provider_called"], true);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(budgets.load(Ordering::SeqCst), 1);
    let opportunity = id(&requested["opportunity_id"]);
    let dispatch: (Uuid, Value) = sqlx::query_as(
        "SELECT id,configuration_snapshot FROM advisory_dispatch WHERE opportunity_id=$1",
    )
    .bind(opportunity)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        dispatch.1["matrix_authority"]["frozen_snapshot_id"],
        snapshot
    );
    assert_eq!(
        dispatch.1["matrix_authority"]["verification_digest"],
        verified["verification_digest"]
    );
    let correlated: (Uuid, Uuid, String) = sqlx::query_as(
        "SELECT a.opportunity_id,a.dispatch_id,o.matrix_verification_digest FROM advisory_matrix_advice a JOIN advisory_opportunity o ON o.id=a.opportunity_id WHERE a.opportunity_id=$1")
        .bind(opportunity).fetch_one(&pool).await.unwrap();
    assert_eq!((correlated.0, correlated.1), (opportunity, dispatch.0));
    assert_eq!(
        correlated.2,
        verified["verification_digest"].as_str().unwrap()
    );

    propose_confirm(&mut owner, program, 2, declarations("mvp")).await;
    let changed = advice(&mut owner, task, &format!("changed-{}", Uuid::new_v4())).await;
    assert_eq!(changed["state"], "no_call", "{changed}");
    assert_eq!(changed["reason"], "matrix_context_stale");
    assert_eq!(changed["provider_called"], false);
    assert_eq!(
        (calls.load(Ordering::SeqCst), budgets.load(Ordering::SeqCst)),
        (1, 1)
    );
    let changed_dispatches: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE opportunity_id=$1")
            .bind(id(&changed["opportunity_id"]))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(changed_dispatches, 0);

    propose_confirm(&mut owner, program, 3, declarations("production")).await;
    let production = route(
        &mut owner,
        "query",
        "engineering.matrix.context.effective.get",
        json!({"locator":locator(program)}),
    )
    .await;
    assert_eq!(
        production["values"]["mode"]["value"],
        json!({"kind":"mode","value":"production"})
    );

    let legacy = Uuid::new_v4();
    record(&mut owner, legacy, Uuid::new_v4(), None).await;
    let unbound = advice(&mut owner, legacy, &format!("unbound-{}", Uuid::new_v4())).await;
    assert_eq!(unbound["state"], "no_call", "{unbound}");
    assert_eq!(unbound["reason"], "matrix_task_unbound");
    assert_eq!(
        (calls.load(Ordering::SeqCst), budgets.load(Ordering::SeqCst)),
        (1, 1)
    );
    let unbound_dispatches: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE opportunity_id=$1")
            .bind(id(&unbound["opportunity_id"]))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(unbound_dispatches, 0);
    independent.finish().await;
    owner.finish().await;
    server.abort();
}

/// A separate, fresh fixture for the session-preference authorization path.
/// The original checkpoint test above remains pinned to its own historical DB.
async fn session_positive_disposable_pair() -> (PgPool, String) {
    const DATABASE: &str = "tect_matrix_session_positive";
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let admin_options = PgConnectOptions::from_str(&admin_url).unwrap();
    let runtime_options = PgConnectOptions::from_str(&runtime_url).unwrap();
    assert_eq!(admin_options.get_username(), "postgres");
    assert_eq!(runtime_options.get_username(), "tect_ci");
    assert_eq!(admin_options.get_database(), Some(DATABASE));
    assert_eq!(runtime_options.get_database(), Some(DATABASE));
    assert_eq!(admin_options.get_host(), "127.0.0.1");
    assert_eq!(runtime_options.get_host(), "127.0.0.1");
    assert_eq!(admin_options.get_port(), 65527);
    assert_eq!(runtime_options.get_port(), 65527);
    let pool = PgPool::connect_with(admin_options).await.unwrap();
    let identity: (i32, String, String, String, bool) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),current_user,\
         (SELECT system_identifier::text FROM pg_control_system()),\
         to_regclass('public._sqlx_migrations') IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        identity,
        (
            180006,
            DATABASE.into(),
            "postgres".into(),
            "7690404534065724697".into(),
            true,
        )
    );
    let runtime: (String, String) = sqlx::query_as("SELECT current_database(),current_user")
        .fetch_one(&PgPool::connect_with(runtime_options).await.unwrap())
        .await
        .unwrap();
    assert_eq!(runtime, (DATABASE.into(), "tect_ci".into()));
    (pool, runtime_url)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "writes only exact owned disposable PostgreSQL 18.6 fixture"]
async fn two_owner_sessions_skip_before_matrix_authorization_and_default_sends_once() {
    let (pool, runtime_url) = session_positive_disposable_pair().await;
    admin::migrate(&pool, "tect_ci").await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("matrix-session-positive.sock");
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let workspace_key = format!("matrix-session-positive-{}", Uuid::new_v4());
    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
    let (workspace, owner_keys) = signed_fixture_budget(&store, &enrolled, &workspace_key).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let budgets = Arc::new(AtomicUsize::new(0));
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(store.with_budget_owner_keys(owner_keys)),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(Evidence))
        .with_matrix_advisory_adapters(
            Arc::new(Provider(calls.clone())),
            Arc::new(Budget(budgets.clone())),
        ),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    let owner_host = root.join("owner.json");
    host_file(&owner_host, &enrolled.auth);
    let mut owner_a = Mcp::start(
        &socket,
        &owner_host,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    let mut owner_b = Mcp::start(
        &socket,
        &owner_host,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    let (source, _) = ready_source_candidate(&mut owner_a, &repo).await;
    let opened_b = owner_b.call("open_workspace", json!({})).await;
    assert_eq!(id(&opened_b["workspace"]["id"]), workspace);
    let program: Uuid =
        sqlx::query_scalar("SELECT program_id FROM scope_candidate_sets WHERE id=$1")
            .bind(id(&source["candidate_set"]["id"]))
            .fetch_one(&pool)
            .await
            .unwrap();
    propose_confirm(&mut owner_a, program, 0, declarations("demo")).await;
    let effective = route(
        &mut owner_a,
        "query",
        "engineering.matrix.context.effective.get",
        json!({"locator":locator(program)}),
    )
    .await;
    let task = Uuid::new_v4();
    let recorded = record(&mut owner_a, task, Uuid::new_v4(), Some(locator(program))).await;
    route(
        &mut owner_a,
        "command",
        "workspace.advisory.configure",
        json!({"expected_revision":0,"mode":"optional",
            "provider_profile_ref":{"id":PROFILE},"model_configuration":{"model":MODEL}}),
    )
    .await;
    let verifier = admin::prepare_verifier_enrollment(&pool, enrolled.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let verifier_host = root.join("verifier.json");
    host_file(&verifier_host, &verifier.auth);
    let mut independent = Mcp::start(
        &socket,
        &verifier_host,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    independent.call("open_workspace", json!({})).await;
    let parsed: EngineeringMatrixInput = serde_json::from_value(recorded["input"].clone()).unwrap();
    let context: EffectiveMatrixRequirements = serde_json::from_value(effective).unwrap();
    let evidence: Vec<_> = required_matrix_operating_facts(&context, &parsed)
        .unwrap()
        .iter()
        .map(|fact| {
            json!({"fact_path":fact.path,
                "evidence_ref":format!("urn:synthetic:session:{}", fact.path)})
        })
        .collect();
    route(
        &mut independent,
        "command",
        "engineering.matrix.verify",
        json!({"task_id":task,"expected_revision":1,
            "input_digest":recorded["input_digest"],"evidence":evidence}),
    )
    .await;

    route(
        &mut owner_a,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":0,"preference":"skip"}),
    )
    .await;
    let skipped_key = format!("session-a-skip-{}", Uuid::new_v4());
    let skipped = advice(&mut owner_a, task, &skipped_key).await;
    assert_eq!(skipped["state"], "no_call");
    assert_eq!(skipped["reason"], "session_skip");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(budgets.load(Ordering::SeqCst), 0);
    let no_dispatches: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE opportunity_id=$1")
            .bind(id(&skipped["opportunity_id"]))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(no_dispatches, 0);

    let sent_key = format!("session-b-default-{}", Uuid::new_v4());
    let sent = advice(&mut owner_b, task, &sent_key).await;
    assert_eq!(sent["state"], "advised", "{sent}");
    assert_eq!(sent["provider_called"], true);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(budgets.load(Ordering::SeqCst), 1);
    let snapshots: Vec<(Uuid, String, String, String)> = sqlx::query_as(
        "SELECT session_id,request_key,session_preference,request_preference \
         FROM advisory_opportunity WHERE id=$1 OR id=$2 ORDER BY request_key",
    )
    .bind(id(&skipped["opportunity_id"]))
    .bind(id(&sent["opportunity_id"]))
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(snapshots.len(), 2);
    assert_ne!(snapshots[0].0, snapshots[1].0);
    assert!(
        snapshots
            .iter()
            .any(|row| row.1 == skipped_key && row.2 == "skip")
    );
    assert!(
        snapshots
            .iter()
            .any(|row| row.1 == sent_key && row.2 == "use_workspace")
    );
    assert!(snapshots.iter().all(|row| row.3 == "use_workspace"));

    let replay = advice(&mut owner_b, task, &sent_key).await;
    assert_eq!(replay["opportunity_id"], sent["opportunity_id"]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let wrong_session = route_error(
        &mut owner_a,
        "command",
        "engineering.advisory.request",
        json!({"task_id":task,"expected_task_revision":1,"request_key":sent_key}),
    )
    .await;
    assert_eq!(wrong_session["error"]["code"], "input_conflict");
    independent.finish().await;
    owner_a.finish().await;
    owner_b.finish().await;
    server.abort();
}
