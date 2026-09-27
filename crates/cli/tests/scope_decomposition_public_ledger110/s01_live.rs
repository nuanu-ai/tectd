//! Ignored, test-only S01 one-shot. `preflight` has no credential or HTTP path.
use super::*;
use sha2::Sha256;
use std::{
    fs,
    io::{BufRead, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};
use tect_application::{
    AdvisoryProviderReceiptUsage, ScopeAdviceRawObservation, ScopeAuthoredManifestRequest,
    ScopeAuthorityObserver, ScopeAuthorityOutcome, ScopeAuthorityRequest, ScopeManifestSupplier,
    Sha256ScopeDigest, StoredAdvisoryProviderReceipt,
};
use tect_domain::{NormalizedScopeAdviceAnswers, ScopeAdviceRequest};

#[path = "s01_live/audit.rs"]
mod audit;
#[path = "s01_live/loopback.rs"]
mod loopback;

const CALL_ID: &str = "tectd-jev-s01-evidence-2026-09-28-2";
const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const MODEL: &str = "jev-1.13.0";
const ARTIFACT_DIR: &str =
    "/Users/tony/Work/Projects/nuanu-ai-lab/artifacts/jev-live-eval-20260919";
const MAX_REQUEST: usize = 45_000;

fn artifact(name: &str) -> std::path::PathBuf {
    Path::new(ARTIFACT_DIR).join(format!("{CALL_ID}.{name}"))
}

fn exclusive_file(path: &Path, bytes: &[u8]) {
    assert!(path.is_absolute() && path.parent().unwrap().is_dir());
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .expect("one-shot artifact exists or cannot be created; no call sent");
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    fs::File::open(path.parent().unwrap())
        .unwrap()
        .sync_all()
        .unwrap();
    assert_eq!(fs::read(path).unwrap(), bytes);
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

fn confirmation(reader: &mut impl BufRead, digest: &str) -> bool {
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return false;
    }
    line.strip_suffix('\n')
        .is_some_and(|s| s.strip_suffix('\r').unwrap_or(s) == format!("SEND JEV {digest}"))
}

#[test]
fn one_shot_guard_requires_exact_line_and_exclusive_artifact() {
    let dir = private_temp();
    let path = dir.path().join("test.used");
    for line in ["", "SEND JEV abc", "SEND JEV wrong\n", "send jev abc\n"] {
        assert!(!confirmation(&mut std::io::Cursor::new(line), "abc"));
        assert!(!path.exists());
    }
    assert!(confirmation(
        &mut std::io::Cursor::new("SEND JEV abc\n"),
        "abc"
    ));
    exclusive_file(&path, b"call_id=test\nrequest_sha256=abc\n");
    assert!(std::panic::catch_unwind(|| exclusive_file(&path, b"replacement")).is_err());
    assert_eq!(
        fs::read(path).unwrap(),
        b"call_id=test\nrequest_sha256=abc\n"
    );
}

struct OneUseBudget {
    signed: AdvisoryBudgetPolicy,
    evaluations: Arc<AtomicUsize>,
}
#[async_trait]
impl ScopeBudgetPolicy for OneUseBudget {
    async fn evaluate(
        &self,
        _: &ScopeBudgetRequest,
        policy: &AdvisoryBudgetPolicy,
    ) -> tect_domain::Result<Option<ScopeBudgetPolicyEvaluation>> {
        self.evaluations.fetch_add(1, Ordering::SeqCst);
        if *policy != self.signed {
            return Ok(None);
        }
        Ok(Some(ScopeBudgetPolicyEvaluation {
            policy_id: policy.id().to_string(),
            policy_version: policy.version(),
            policy_digest: policy.digest().to_owned(),
        }))
    }
}

struct ReviewedProvider {
    inner: tect_host::JevScopeAdviceProvider,
    body: Vec<u8>,
}
#[async_trait]
impl ScopeAdviceProvider for ReviewedProvider {
    fn identity(&self) -> Option<(&'static str, &'static str)> {
        self.inner.identity()
    }
    fn prepare_context(
        &self,
        context: &ScopeAdviceProviderContext,
    ) -> Result<PreparedScopeAdviceAttempt, ScopeAdviceProviderError> {
        let prepared = self.inner.prepare_context(context)?;
        if prepared.body() != self.body {
            return Err(ScopeAdviceProviderError::ProvenNotSent);
        }
        Ok(prepared)
    }
    async fn attempt_prepared(
        &self,
        _: &ScopeAdviceProviderRequest,
        _: PreparedScopeAdviceAttempt,
        _: StartedScopeDispatchPermit,
    ) -> Result<ScopeAdviceProviderObservation, ScopeAdviceProviderError> {
        Err(ScopeAdviceProviderError::ProvenNotSent)
    }
    async fn observe_prepared(
        &self,
        request: &ScopeAdviceProviderRequest,
        prepared: PreparedScopeAdviceAttempt,
        permit: StartedScopeDispatchPermit,
    ) -> Result<ScopeAdviceRawObservation, ScopeAdviceProviderError> {
        if prepared.body() != self.body {
            return Err(ScopeAdviceProviderError::ProvenNotSent);
        }
        self.inner.observe_prepared(request, prepared, permit).await
    }
    fn usage_from_sealed_response(
        &self,
        saved: &StoredAdvisoryProviderReceipt,
    ) -> tect_domain::Result<AdvisoryProviderReceiptUsage> {
        self.inner.usage_from_sealed_response(saved)
    }
    fn parse_sealed_response(
        &self,
        prepared: &PreparedScopeAdviceAttempt,
        saved: &StoredAdvisoryProviderReceipt,
    ) -> tect_domain::Result<NormalizedScopeAdviceAnswers> {
        self.inner.parse_sealed_response(prepared, saved)
    }
}

fn provider(key: String) -> tect_host::JevScopeAdviceProvider {
    provider_at(ENDPOINT, key)
}

fn provider_at(endpoint: &str, key: String) -> tect_host::JevScopeAdviceProvider {
    tect_host::JevScopeAdviceProvider::new(
        tect_host::JevScopeAdviceConfig {
            profile: CALL_ID.into(),
            endpoint: endpoint.parse().unwrap(),
            model: MODEL.into(),
            timeout: std::time::Duration::from_secs(15),
            maximum_request_bytes: MAX_REQUEST,
            maximum_response_bytes: 65_536,
        },
        key,
    )
    .unwrap()
}

async fn fresh_database() -> (PgPool, String) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let system = std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").unwrap();
    let oid: i64 = std::env::var("TECT_TEST_EXPECTED_DB_OID")
        .unwrap()
        .parse()
        .unwrap();
    let port: u16 = std::env::var("TECT_TEST_EXPECTED_PG_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    assert_eq!(role, "tect_ci");
    let database = std::env::var("TECT_TEST_EXPECTED_DB_NAME").unwrap();
    assert!(database.starts_with("tect_s01_live_"));
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    for (url, user) in [(&admin_url, "postgres"), (&runtime_url, "tect_ci")] {
        let options = PgConnectOptions::from_str(url).unwrap();
        assert_eq!(options.get_username(), user);
        assert_eq!(options.get_database(), Some(database.as_str()));
        assert_eq!(options.get_host(), "127.0.0.1");
        assert_eq!(options.get_port(), port);
        assert!(options.get_socket().is_none());
    }
    let pool = PgPool::connect(&admin_url).await.unwrap();
    let identity: (i32, String, String, i64, String) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),current_user, \
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()), \
         (SELECT system_identifier::text FROM pg_control_system())",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        identity,
        (180006, database.clone(), "postgres".into(), oid, system)
    );
    let runtime = PgPool::connect(&runtime_url).await.unwrap();
    let runtime_identity: (String, String, i64) = sqlx::query_as(
        "SELECT current_database(),current_user, \
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database())",
    )
    .fetch_one(&runtime)
    .await
    .unwrap();
    assert_eq!(runtime_identity, (database, role.clone(), oid));
    drop(runtime);
    let residue: i64 = sqlx::query_scalar("SELECT \
        (SELECT count(*) FROM pg_namespace WHERE nspname NOT IN ('pg_catalog','pg_toast','public','information_schema')) + \
        (SELECT count(*) FROM pg_class WHERE relnamespace='public'::regnamespace) + \
        (SELECT count(*) FROM pg_proc WHERE pronamespace='public'::regnamespace) + \
        (SELECT count(*) FROM pg_type WHERE typnamespace='public'::regnamespace) + \
        (SELECT count(*) FROM pg_extension WHERE extname <> 'plpgsql') + \
        (SELECT count(*) FROM pg_event_trigger) + (SELECT count(*) FROM pg_publication) + \
        (SELECT count(*) FROM pg_subscription)")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(
        residue, 0,
        "dedicated S01 database must be empty before migration"
    );
    admin::migrate(&pool, &role).await.unwrap();
    (pool, runtime_url)
}

async fn s01_source_candidate(client: &mut Mcp, source: &Path) -> (Value, Value) {
    client.call("open_workspace", json!({})).await;
    let registered = client.call("register_source", json!({"path":source})).await;
    client
        .call(
            "select_worktrees",
            json!({"worktree_ids":[registered["id"]]}),
        )
        .await;
    let begun = client.call("begin_program", json!({
        "request_id":Uuid::new_v4(),
        "input":"From the exact saved source, author complete cohesive and partitioned Scope alternatives. Preserve every applicable source fragment in each alternative and let Jev rank only the eligible authored set. Record an explicit caller disposition; before any selected save, recheck source, configuration, policy and preservation. Use the existing authorized caller and a distinct Verifier, retaining their separate audit receipts."
    })).await;
    let program = client.call("save_program", json!({
        "program_id":begun["program"]["id"],"revision":1,"input_cursor":1,
        "name":"Scope decomposition advice",
        "intent":"Compare complete source-authored Scope alternatives through optional guarded ranking and explicit disposition",
        "basis":"The saved source and each applicable source fragment govern both authored alternatives",
        "boundaries":"Jev ranks only eligible alternatives; advice cannot select or mutate by itself",
        "constraints":"Recheck source, configuration, policy and preservation before the existing caller; keep caller and distinct Verifier receipts",
        "success":"A complete alternative can be explicitly selected, saved through the existing caller and independently observed by a Verifier; nonselective advice can be rejected",
        "complete":true
    })).await;
    let candidates = client.call("begin_candidate_set", json!({
        "request_id":Uuid::new_v4(),"program_id":program["program"]["id"],
        "program_revision":program["program"]["revision"],"boundary":"ongoing",
        "input":"Compare one cohesive Scope with two dependent partitioned Scopes for the same complete S01 work."
    })).await;
    let context = &candidates["context"];
    let inputs = client
        .call(
            "candidate_context",
            json!({
                "candidate_set_id":context["candidate_set"]["id"],"view":"inputs","limit":25
            }),
        )
        .await;
    let source_ref = &inputs["items"][0]["input"]["source_ref_id"];
    let saved = client.call("save_candidate_set", json!({
        "kind":"draft","candidate_set_id":context["candidate_set"]["id"],"revision":1,
        "snapshot_id":context["snapshot"]["id"],"input_cursor":1,"request_id":Uuid::new_v4(),
        "draft":{"boundary":"ongoing","goals":[{
            "identity":{"local":"goal"},"text":"Complete source-preserving Scope advice and explicit effect path",
            "source_ref_id":source_ref,"resolution":{"kind":"candidate","reference":{"local":"scope"}}
        }],"evidence":[],"candidates":[{
            "identity":{"local":"scope"},"title":"Scope advice and disposition",
            "outcome":"Complete alternatives can be ranked and explicitly disposed",
            "trigger":"An exact source and candidate set are available",
            "delivered_behavior":"Guarded advice, explicit decision and existing caller path",
            "proof":"Source coverage and separate caller and Verifier receipts",
            "includes":["authored alternatives","ranking","disposition","existing caller and Verifier"],
            "excludes":["automatic effect","new mutation engine"],"dependencies":[],
            "coverage_goals":[{"local":"goal"}],"evidence":[]
        }],"blockers":[],"protected_changes":[]}
    })).await;
    let candidate = saved["draft"]["candidates"][0].clone();
    let reviewed = client
        .call(
            "save_candidate_set",
            json!({
                "kind":"review","candidate_set_id":saved["context"]["candidate_set"]["id"],
                "revision":saved["context"]["candidate_set"]["revision"],
                "snapshot_id":context["snapshot"]["id"],
                "input_cursor":saved["context"]["candidate_set"]["input_cursor"],
                "request_id":Uuid::new_v4(),
                "review":{"verdict":"ready","summary":"Complete Scope advice path",
                    "findings":[],"candidate_decisions":[{"candidate_id":candidate["id"],
                    "decision":"accept","rationale":"Complete source-backed Scope"}]}
            }),
        )
        .await;
    (reviewed["context"].clone(), candidate)
}

fn cohesive_draft(source_ref: Uuid, prior: &Value) -> Value {
    json!({"boundary":"ongoing","goals":[{
        "identity":{"local":"complete"},
        "text":"Preserve the complete authored set, ranking-only advice, explicit disposition and authorized save with separate Verifier observation",
        "source_ref_id":source_ref,
        "resolution":{"kind":"candidate","reference":{"local":"scope"}}
    }],"evidence":[],"candidates":[{
        "identity":{"local":"scope"},"title":"Scope decomposition advice and effect path",
        "outcome":"A complete source-covering alternative is explicitly chosen or rejected, and any selected save has caller and Verifier evidence",
        "trigger":"An exact source and current candidate set are available",
        "delivered_behavior":"Author and preserve complete alternatives; audit the exact request and optional provider ranking; record caller disposition; recheck source, configuration, policy and preservation before the existing caller; observe its result with a distinct Verifier",
        "proof":"The manifest covers every applicable source fragment, the request and dispatch are auditable, and disposition, preservation, caller and Verifier receipts remain separate",
        "includes":["complete source-authored alternatives and preservation","exact request and provider audit","ranking-only guarded advice and explicit disposition","source/configuration/policy/preservation recheck","existing caller and distinct Verifier"],
        "excludes":["automatic selection","new mutation engine","unrelated deployment"],
        "dependencies":[],"coverage_goals":[{"local":"complete"}],"evidence":[]
    }],"blockers":[],"protected_changes":[],"supersessions":[{
        "candidate_id":prior["id"],"revision":prior["revision"],
        "reason":"Compare complete authored Scope arrangements for the same source",
        "replacements":[{"local":"scope"}]
    }]})
}

fn partitioned_draft(source_ref: Uuid, prior: &Value) -> Value {
    json!({"boundary":"ongoing","goals":[
        {"identity":{"local":"preparation"},"text":"Preserve complete authored alternatives and audit the exact ranking request", "source_ref_id":source_ref,
         "resolution":{"kind":"candidate","reference":{"local":"prepare"}}},
        {"identity":{"local":"effect_goal"},"text":"Explicitly dispose guarded advice and use the existing authorized caller and distinct Verifier", "source_ref_id":source_ref,
         "resolution":{"kind":"candidate","reference":{"local":"effect"}}}
    ],"evidence":[],"candidates":[
        {"identity":{"local":"prepare"},"title":"Source-preserving alternatives and request audit",
         "outcome":"Complete eligible alternatives and their exact optional ranking request are auditable", "trigger":"An exact source and current candidate set are available",
         "delivered_behavior":"Author and freeze complete alternatives, cover every applicable source fragment, build the exact request, and audit any single provider ranking attempt",
         "proof":"The manifest covers every applicable fragment and retained request, digest, dispatch and provider response identify the attempted ranking",
         "includes":["complete source-authored alternatives and preservation","exact request and provider audit"],
         "excludes":["automatic selection","new mutation engine","unrelated deployment"],
         "dependencies":[],"coverage_goals":[{"local":"preparation"}],"evidence":[]},
        {"identity":{"local":"effect"},"title":"Explicit disposition and existing effect path",
         "outcome":"Guarded advice is explicitly accepted or rejected, and any selected save has caller and Verifier evidence",
         "trigger":"Complete alternatives and either an audited ranking result or an audited deterministic baseline for a disabled, skipped, or failed provider attempt are available",
         "delivered_behavior":"Record explicit disposition; recheck source, configuration, policy and preservation before the existing authorized caller; observe its result with a distinct Verifier",
         "proof":"Advice and disposition identify the selection or rejection, while preservation, caller and Verifier receipts remain separate",
         "includes":["ranking-only guarded advice and explicit disposition","source/configuration/policy/preservation recheck","existing caller and distinct Verifier"],
         "excludes":["automatic effect","new mutation engine","unrelated deployment"],
         "dependencies":[{"local":"prepare"}],"coverage_goals":[{"local":"effect_goal"}],"evidence":[]}
    ],"blockers":[],"protected_changes":[],"supersessions":[{
        "candidate_id":prior["id"],"revision":prior["revision"],
        "reason":"Compare complete authored Scope arrangements for the same source",
        "replacements":[{"local":"prepare"},{"local":"effect"}]
    }]})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "explicit JEV_S01_ONE_SHOT_MODE=preflight or send; fresh dedicated PG18 only"]
async fn real_s01_one_shot() {
    let mode = std::env::var("JEV_S01_ONE_SHOT_MODE").expect("explicit preflight or send mode");
    assert!(matches!(mode.as_str(), "preflight" | "send"));
    run_fixture(&mode).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "fresh dedicated PG18; local loopback only, no external provider"]
async fn loopback_selective_native_receipt() {
    run_fixture("loopback_selective").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "fresh dedicated PG18; local loopback only, no external provider"]
async fn loopback_nonselective_native_receipt() {
    run_fixture("loopback_nonselective").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "fresh dedicated PG18; mismatched reviewed bytes must block before loopback send"]
async fn loopback_rejects_changed_reviewed_body() {
    run_fixture("loopback_negative").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "fresh dedicated PG18; local HTTP 500 receipt must be audited without advice"]
async fn loopback_failure_retains_transport_audit() {
    run_fixture("loopback_failure").await;
}

async fn run_fixture(mode: &str) {
    let loopback = mode.starts_with("loopback_");
    let request_path = artifact(if mode == "preflight" {
        "preflight-native-verified.request.json"
    } else if mode == "loopback_selective" {
        "loopback-selective.request.json"
    } else if mode == "loopback_nonselective" {
        "loopback-nonselective-v2.request.json"
    } else if mode == "loopback_negative" {
        "loopback-negative.request.json"
    } else if mode == "loopback_failure" {
        "loopback-failure-v2.request.json"
    } else {
        "request.json"
    });
    let marker_path = artifact("used");
    assert!(
        !request_path.exists() && !marker_path.exists(),
        "one-use artifact already exists"
    );
    let (pool, runtime_url) = fresh_database().await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let workspace_key = format!("s01-live-{}", Uuid::new_v4());
    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
    // Scope v1 schema requires retry_dispatches >= 1. The provider itself has
    // zero retries and this one-shot fixture never invokes a retry route.
    let ceilings = AdvisoryBudgetCeilings {
        provider_calls: 1,
        input_tokens: 24_000,
        output_tokens: 2_000,
        request_utf8_bytes: MAX_REQUEST as i64,
        elapsed_monotonic_ms: 60_000,
        retry_dispatches: 1,
    };
    let (workspace, keys) =
        signed_fixture_budget(&store, &enrolled, &workspace_key, ceilings).await;
    let trusted = store.with_budget_owner_keys(keys);
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let mut policy_tx = trusted.begin(TransactionMode::ReadOnly).await.unwrap();
    policy_tx.authenticate(&enrolled.auth).await.unwrap();
    policy_tx.set_tenant(enrolled.tenant_id).await.unwrap();
    let authorized = policy_tx
        .advisory_budget_policy_store()
        .unwrap()
        .authorized_budget_policy(workspace, now)
        .await
        .unwrap()
        .expect("signed owner policy must verify under runtime trust key");
    assert_eq!(authorized.ceilings(), ceilings);
    assert_eq!(authorized.approved_by(), enrolled.principal_id);
    assert!(authorized.is_effective_at(now));
    policy_tx.commit().await.unwrap();
    let authority = Arc::new(PgScopeAuthorityObserver::new(
        trusted.clone(),
        Arc::new(tect_host::StaticCandidateGuidance),
    ));
    let supplier = Arc::new(PgScopeAuthoredManifestSupplier::new(
        trusted.clone(),
        authority.clone(),
    ));
    let context = tect_domain::RequestContext {
        auth: enrolled.auth.clone(),
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: workspace_key.clone(),
    };
    let socket = root.join("setup.sock");
    let setup_service = Arc::new(WorkspaceService::new_with_scope_advisory_adapters(
        Arc::new(trusted.clone()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
        authority.clone(),
        supplier.clone(),
        Arc::new(tect_application::DenyScopeBudget),
        Arc::new(provider("preflight-placeholder-never-sent".into())),
    ));
    let listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    let setup_server = tokio::spawn(tect_host::serve(listener, setup_service));
    let host = root.join("host.json");
    host_file(&host, &enrolled.auth);
    let mut owner = Mcp::start(&socket, &host, &context.native_session_id, &workspace_key).await;
    let (source, prior) = s01_source_candidate(&mut owner, &repo).await;
    let candidate_set = id(&source["candidate_set"]["id"]);
    let revision = source["candidate_set"]["revision"].as_i64().unwrap();
    let snapshot = source["snapshot"]["id"].clone();
    let input_cursor = source["candidate_set"]["input_cursor"].as_i64().unwrap();
    let configured = route(
        &mut owner,
        "command",
        "workspace.advisory.configure",
        json!({
        "expected_revision":0,"mode":"optional","provider_profile_ref":{"id":CALL_ID},
        "model_configuration":{"model":MODEL}}),
    )
    .await;
    assert_eq!(id(&configured["workspace_id"]), workspace);
    let inputs = owner
        .call(
            "candidate_context",
            json!({"candidate_set_id":candidate_set,"view":"inputs","limit":25}),
        )
        .await;
    let program = owner
        .call(
            "candidate_context",
            json!({"candidate_set_id":candidate_set,"view":"program","limit":25}),
        )
        .await;
    let mut refs: Vec<Uuid> = inputs["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| id(&x["input"]["source_ref_id"]))
        .collect();
    let source_ref = *refs.first().unwrap();
    refs.extend(
        program["program"]["field_refs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| id(&x["id"])),
    );
    refs.sort_unstable();
    refs.dedup();
    let cohesive = cohesive_draft(source_ref, &prior);
    let partitioned = partitioned_draft(source_ref, &prior);
    let authored = tect_application::AuthoredScopeSet {
        expected_candidate_set_revision: revision,
        baseline_key: "cohesive".into(),
        alternatives: vec![
            tect_application::AuthoredScopeAlternative {
                key: "cohesive".into(),
                kind: tect_domain::ScopeDecompositionKind::Cohesive,
                draft: serde_json::from_value(cohesive.clone()).unwrap(),
                covered_source_ref_ids: refs.clone(),
            },
            tect_application::AuthoredScopeAlternative {
                key: "partitioned".into(),
                kind: tect_domain::ScopeDecompositionKind::Partitioned,
                draft: serde_json::from_value(partitioned.clone()).unwrap(),
                covered_source_ref_ids: refs,
            },
        ],
    };
    authored.validate().unwrap();
    let (session_id, actor_id): (Uuid, Uuid) = sqlx::query_as(
        "SELECT s.id,h.principal_id FROM agent_sessions s JOIN hosts h ON (s.tenant_id,s.host_id)=(h.tenant_id,h.id) WHERE s.tenant_id=$1 AND s.native_session_id=$2")
        .bind(enrolled.tenant_id).bind(&context.native_session_id).fetch_one(&pool).await.unwrap();
    let observation = authority
        .observe(&ScopeAuthorityRequest {
            tenant_id: enrolled.tenant_id,
            workspace_id: workspace,
            actor_id,
            session_id,
            candidate_set_id: candidate_set,
        })
        .await
        .unwrap();
    let ScopeAuthorityOutcome::Authorized(observation) = observation else {
        panic!("source not authorized")
    };
    let manifest = supplier
        .supply_authored(&ScopeAuthoredManifestRequest {
            tenant_id: enrolled.tenant_id,
            observation: *observation,
            authored_scope_set: authored.clone(),
        })
        .await
        .unwrap();
    manifest.validate(&Sha256ScopeDigest).unwrap();
    assert_eq!(manifest.emitted.len(), 2);
    let obligations = manifest
        .obligations
        .iter()
        .map(|x| x.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(!obligations.is_empty());
    for alternative in &manifest.emitted {
        assert!(!alternative.material.candidates.is_empty());
        assert_eq!(
            alternative
                .coverage
                .iter()
                .map(|x| x.obligation_id.as_str())
                .collect::<std::collections::BTreeSet<_>>(),
            obligations
        );
    }
    assert_eq!(
        manifest
            .emitted
            .iter()
            .find(|x| x.kind == tect_domain::ScopeDecompositionKind::Cohesive)
            .unwrap()
            .material
            .candidates
            .len(),
        1
    );
    let partitioned_material = &manifest
        .emitted
        .iter()
        .find(|x| x.kind == tect_domain::ScopeDecompositionKind::Partitioned)
        .unwrap()
        .material
        .candidates;
    assert_eq!(partitioned_material.len(), 2);
    assert!(partitioned_material[0].dependencies.is_empty());
    assert_eq!(
        partitioned_material[1].dependencies,
        vec![partitioned_material[0].id]
    );
    let request = ScopeAdviceRequest::from_manifest(&Sha256ScopeDigest, &manifest).unwrap();
    let provider_context = ScopeAdviceProviderContext::from_manifest(&request, &manifest).unwrap();
    let prepared = provider("preflight-placeholder-never-sent".into())
        .prepare_context(&provider_context)
        .unwrap();
    assert_eq!(prepared.destination(), ENDPOINT);
    assert_eq!(prepared.model(), MODEL);
    assert!(prepared.body_length() > 0 && prepared.body_length() <= MAX_REQUEST);
    let digest = prepared.body_sha256().to_owned();
    assert_eq!(digest, format!("{:x}", Sha256::digest(prepared.body())));
    let json_body: Value = serde_json::from_slice(prepared.body()).unwrap();
    assert_eq!(
        json_body["state"]["emitted"],
        serde_json::to_value(&manifest.emitted).unwrap()
    );
    assert_eq!(
        json_body["state"]["request"],
        serde_json::to_value(&request).unwrap()
    );
    let no_call = route(
        &mut owner,
        "command",
        "scope.advisory.request",
        json!({
        "request_id":Uuid::new_v4(),"candidate_set_id":candidate_set,
        "authored_scope_set":authored}),
    )
    .await;
    assert_eq!(no_call["state"], "no_call");
    assert_eq!(no_call["reason"], "budget_policy_invalid");
    let dispatches: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(dispatches, 0);
    exclusive_file(&request_path, prepared.body());
    println!(
        "S01 preflight call_id={CALL_ID} workspace={workspace} candidate={candidate_set} bytes={} sha256={digest} no_call={}",
        prepared.body_length(),
        no_call["opportunity_id"]
    );
    if mode == "preflight" {
        owner.finish().await;
        setup_server.abort();
        return;
    }
    // The preflight file is shape evidence from a different random fixture.
    // This invocation freezes fresh bytes and requires their own exact digest.
    let (inner, loopback_server) = if mode == "loopback_negative" {
        (
            provider_at(
                "http://127.0.0.1:9/v1/systemone",
                "local-fixture-only".into(),
            ),
            None,
        )
    } else if loopback {
        let (endpoint, server) = loopback::start(
            &request,
            mode == "loopback_selective",
            mode == "loopback_failure",
        )
        .await;
        (
            provider_at(&endpoint, "local-fixture-only".into()),
            Some(server),
        )
    } else {
        let key = std::env::var("TYPESAFE_API_KEY")
            .expect("send requires process-level TYPESAFE_API_KEY");
        assert!(!key.trim().is_empty());
        (provider(key), None)
    };
    let mut reviewed = ReviewedProvider {
        inner,
        body: fs::read(&request_path).unwrap(),
    };
    if mode == "loopback_negative" {
        reviewed.body.push(b'!');
        assert!(matches!(
            reviewed.prepare_context(&provider_context),
            Err(ScopeAdviceProviderError::ProvenNotSent)
        ));
    } else {
        assert_eq!(
            reviewed.prepare_context(&provider_context).unwrap().body(),
            prepared.body()
        );
    }
    if !loopback {
        println!(
            "Review this invocation's exact request {} ({} bytes, sha256={digest}); preflight bytes differ. Enter exactly: SEND JEV {digest}",
            request_path.display(),
            prepared.body_length()
        );
        std::io::stdout().flush().unwrap();
        assert!(
            confirmation(&mut std::io::stdin().lock(), &digest),
            "confirmation absent or mismatched; no send"
        );
        exclusive_file(
            &marker_path,
            format!("call_id={CALL_ID}\nrequest_sha256={digest}\n").as_bytes(),
        );
    }
    owner.finish().await;
    setup_server.abort();
    let _ = setup_server.await;
    let live_socket = root.join("live.sock");
    let budget_evaluations = Arc::new(AtomicUsize::new(0));
    let live_service = Arc::new(WorkspaceService::new_with_scope_advisory_adapters(
        Arc::new(trusted),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
        authority,
        supplier,
        Arc::new(OneUseBudget {
            signed: authorized,
            evaluations: budget_evaluations.clone(),
        }),
        Arc::new(reviewed),
    ));
    let listener = UnixListener::bind(&live_socket).unwrap();
    fs::set_permissions(&live_socket, fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, live_service));
    let mut owner = Mcp::start(
        &live_socket,
        &host,
        &context.native_session_id,
        &workspace_key,
    )
    .await;
    route(&mut owner, "command", "workspace.open", json!({})).await;
    let advised = route(&mut owner, "command", "scope.advisory.request", json!({
        "request_id":Uuid::new_v4(),"candidate_set_id":candidate_set,"authored_scope_set":authored})).await;
    if mode == "loopback_negative" {
        assert_eq!(advised["state"], "no_call");
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1")
                .bind(workspace)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0);
        assert!(!marker_path.exists());
        owner.finish().await;
        server.abort();
        return;
    }
    if let Some(server) = loopback_server {
        let captured = tokio::time::timeout(std::time::Duration::from_secs(20), server)
            .await
            .expect("native loopback provider was not called")
            .unwrap();
        assert_eq!(captured, fs::read(&request_path).unwrap());
    }
    let opportunity = id(&advised["opportunity_id"]);
    audit::assert_one_sealed_attempt(
        &pool,
        workspace,
        opportunity,
        &digest,
        &fs::read(&request_path).unwrap(),
    )
    .await;
    let expected_evaluations = if advised["state"] == "advised" { 2 } else { 1 };
    assert!(
        budget_evaluations.load(Ordering::SeqCst) >= expected_evaluations,
        "signed policy must pass every evaluation reached by this outcome"
    );
    let detail = route(
        &mut owner,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":candidate_set,"opportunity_id":opportunity}),
    )
    .await;
    let advice = &detail["scope_decomposition"]["advice"];
    if loopback && mode != "loopback_failure" {
        assert_eq!(advised["state"], "advised", "{advised}");
    }
    if mode == "loopback_failure" {
        assert_ne!(advised["state"], "advised");
        assert!(!advice["items"].is_array());
    }
    if advised["state"] != "advised" || !advice["items"].is_array() {
        println!(
            "S01 provider outcome state={} reason={} dispatch=1; no advice to disposition",
            advised["state"], advised["reason"]
        );
        owner.finish().await;
        server.abort();
        return;
    }
    let emitted = detail["scope_decomposition"]["manifest"]["emitted"]
        .as_array()
        .unwrap();
    let items = advice["items"].as_array().unwrap();
    let preferred = items
        .iter()
        .filter(|x| x["choice"] == "preferred")
        .collect::<Vec<_>>();
    let selective = preferred.len() == 1
        && items
            .iter()
            .filter(|x| x["choice"] == "non_preferred")
            .count()
            == items.len() - 1;
    if mode == "loopback_selective" {
        assert!(selective);
    }
    if mode == "loopback_nonselective" {
        assert!(!selective);
    }
    let selected_id = if selective {
        Some(preferred[0]["alternative_id"].clone())
    } else {
        None
    };
    let states = emitted
        .iter()
        .map(|x| {
            json!({"alternative_id":x["id"],
        "state":if selected_id.as_ref() == Some(&x["id"]) {"selected"} else {"not_selected"}})
        })
        .collect::<Vec<_>>();
    let mut disposition_params = json!({
        "opportunity_id":opportunity,"candidate_set_id":candidate_set,"request_id":Uuid::new_v4(),
        "advice_id":advice["id"],"expected_revision":0,
        "action":if selective {"accept"} else {"reject_all"},
        "items":states,"rationale":if selective {"Select the sole preferred source-covering alternative"}
            else {"JEV advice gives no unique preference; reject its ranking"}});
    if let Some(id) = &selected_id {
        disposition_params["selected_id"] = id.clone();
    }
    let disposition = route(
        &mut owner,
        "command",
        "scope.advisory.disposition",
        disposition_params,
    )
    .await;
    if !selective {
        println!(
            "S01 nonselective advice: reject_all disposition={} ; no selected save",
            disposition["id"]
        );
        owner.finish().await;
        server.abort();
        return;
    }
    let selected_id = selected_id.unwrap();
    let selected = emitted.iter().find(|x| x["id"] == selected_id).unwrap();
    let key = selected["kind"].as_str().unwrap();
    assert!(matches!(key, "cohesive" | "partitioned"));
    let draft = if key == "cohesive" {
        cohesive
    } else {
        partitioned
    };
    let save_request = Uuid::new_v4();
    let saved = route(&mut owner, "command", "scope.candidates.save", json!({
        "kind":"draft","candidate_set_id":candidate_set,"revision":revision,"snapshot_id":snapshot,
        "input_cursor":input_cursor,"request_id":save_request,"selected_advisory":{
            "opportunity_id":opportunity,"disposition_id":disposition["id"],"selected_id":selected_id,
            "alternative_key":key},"draft":draft})).await;
    assert_eq!(
        saved["context"]["candidate_set"]["status"],
        "review_required"
    );
    let (link, target_revision, status): (Uuid, i64, String) = sqlx::query_as(
        "SELECT l.link_id,l.caller_result_revision,p.status FROM advisory_scope_caller_link l JOIN advisory_scope_preservation_receipt p \
         ON (p.tenant_id,p.workspace_id,p.receipt_id)=(l.tenant_id,l.workspace_id,l.preservation_receipt_id) \
         WHERE l.workspace_id=$1 AND l.opportunity_id=$2 AND l.request_id=$3")
        .bind(workspace).bind(opportunity).bind(save_request).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "passed");
    let verifier = admin::prepare_verifier_enrollment(&pool, enrolled.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    assert_ne!(verifier.principal_id, actor_id);
    let verifier_file = root.join("verifier.json");
    host_file(&verifier_file, &verifier.auth);
    let mut verifier_mcp = Mcp::start(
        &live_socket,
        &verifier_file,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    route(&mut verifier_mcp, "command", "workspace.open", json!({})).await;
    let verified = route(&mut verifier_mcp, "command", "candidate.advisory.verify", json!({
        "request_id":Uuid::new_v4(),"opportunity_id":opportunity,"candidate_set_id":candidate_set,
        "caller_link_id":link,"caller_receipt_request_id":save_request,"target_revision":target_revision})).await;
    assert_eq!(verified["observation"]["status"], "passed");
    assert_eq!(
        verified["observation"]["qualification"],
        "independently_observed"
    );
    assert_eq!(
        verified["observation"]["actor_id"],
        verifier.principal_id.to_string()
    );
    println!(
        "S01 selective call={} opportunity={opportunity} disposition={} caller_link={link} verifier={}",
        CALL_ID, disposition["id"], verified["observation"]["id"]
    );
    verifier_mcp.finish().await;
    owner.finish().await;
    server.abort();
}
