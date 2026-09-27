//! Ignored S05 public V2 one-shot. `preflight` never reads a key or opens HTTP.
use super::*;
use std::{
    fs,
    io::{BufRead, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    time::Duration,
};
use tect_application::ModelRouteUsage;
use tect_host::{JevModelRouteConfig, JevModelRouteProvider};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
#[path = "s05_loopback.rs"]
mod loopback;

const CALL_ID_PREFIX: &str = "tectd-jev-s05-v2-2026-09-28-";
const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const ADVISER: &str = "jev-1.13.0";
const ARTIFACT_DIR: &str =
    "/Users/tony/Work/Projects/nuanu-ai-lab/artifacts/jev-live-eval-20260919";
const MAX_REQUEST: usize = 45_000;

fn artifact(suffix: &str) -> std::path::PathBuf {
    Path::new(ARTIFACT_DIR).join(format!("{}.{suffix}", call_id()))
}
fn call_id() -> String {
    let id = std::env::var("S05_ONE_SHOT_CALL_ID").unwrap_or_else(|_| format!("{CALL_ID_PREFIX}2"));
    let suffix = id
        .strip_prefix(CALL_ID_PREFIX)
        .expect("S05 call ID prefix required");
    assert!(!suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()));
    id
}
fn exclusive(path: &Path, body: &[u8]) {
    assert!(path.is_absolute() && path.parent().unwrap().is_dir());
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .expect("one-shot artifact already exists or cannot be created; no send");
    file.write_all(body).unwrap();
    file.sync_all().unwrap();
    fs::File::open(path.parent().unwrap())
        .unwrap()
        .sync_all()
        .unwrap();
    assert_eq!(fs::read(path).unwrap(), body);
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}
fn confirmed(reader: &mut impl BufRead, digest: &str) -> bool {
    let mut line = String::new();
    reader.read_line(&mut line).is_ok()
        && line
            .strip_suffix('\n')
            .is_some_and(|s| s.strip_suffix('\r').unwrap_or(s) == format!("SEND JEV {digest}"))
}
#[test]
fn s05_one_shot_marker_is_exclusive_and_confirmation_exact() {
    let temp = private_temp();
    let marker = temp.path().join("used");
    assert!(!confirmed(&mut std::io::Cursor::new("SEND JEV abc"), "abc"));
    assert!(!confirmed(
        &mut std::io::Cursor::new("SEND JEV bad\n"),
        "abc"
    ));
    assert!(confirmed(
        &mut std::io::Cursor::new("SEND JEV abc\n"),
        "abc"
    ));
    exclusive(&marker, b"call_id=test\nrequest_sha256=abc\n");
    assert!(std::panic::catch_unwind(|| exclusive(&marker, b"again")).is_err());
}

fn provider(key: String) -> JevModelRouteProvider {
    provider_at(ENDPOINT, key)
}
fn provider_at(endpoint: &str, key: String) -> JevModelRouteProvider {
    JevModelRouteProvider::new(
        JevModelRouteConfig {
            profile: call_id(),
            endpoint: endpoint.parse().unwrap(),
            model: ADVISER.into(),
            timeout: Duration::from_secs(10),
            maximum_request_bytes: MAX_REQUEST,
            maximum_response_bytes: 64 * 1024,
        },
        key,
    )
    .unwrap()
}

struct ReviewedProvider {
    inner: JevModelRouteProvider,
    reviewed: Vec<u8>,
    marker: std::path::PathBuf,
}
#[async_trait]
impl ModelRouteRankingProvider for ReviewedProvider {
    fn required_profile(&self) -> Option<&str> {
        self.inner.required_profile()
    }
    fn available(&self) -> bool {
        self.inner.available()
    }
    fn prepare(
        &self,
        saved: &PreparedModelRouteRecommendation,
    ) -> Result<ModelRoutePreparedAttempt> {
        let prepared = self.inner.prepare(saved)?;
        if prepared.request_bytes != self.reviewed || !self.marker.exists() {
            return Err(Error::InputConflict);
        }
        Ok(prepared)
    }
    async fn attempt_prepared(
        &self,
        attempted: ModelRoutePreparedAttempt,
        permit: ModelRouteSendPermit,
    ) -> Result<Vec<u8>> {
        Ok(self.attempt_prepared_observed(attempted, permit).await?.raw)
    }
    async fn attempt_prepared_observed(
        &self,
        attempted: ModelRoutePreparedAttempt,
        permit: ModelRouteSendPermit,
    ) -> Result<ModelRouteProviderObservation> {
        if attempted.request_bytes != self.reviewed || !self.marker.exists() {
            return Err(Error::InputConflict);
        }
        self.inner
            .attempt_prepared_observed(attempted, permit)
            .await
    }
    fn sealed_usage(
        &self,
        attempted: &ModelRoutePreparedAttempt,
        observed: &ModelRouteProviderObservation,
    ) -> Result<ModelRouteUsage> {
        self.inner.sealed_usage(attempted, observed)
    }
    fn parse_sealed(
        &self,
        attempted: &ModelRoutePreparedAttempt,
        observed: &ModelRouteProviderObservation,
    ) -> Result<tect_domain::ModelRouteRankingWireOutcome> {
        self.inner.parse_sealed(attempted, observed)
    }
}

async fn fresh_database() -> (PgPool, String) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let name = std::env::var("TECT_TEST_EXPECTED_DB_NAME").unwrap();
    assert!(name.starts_with("tect_s05_live_"));
    let expected_oid: i64 = std::env::var("TECT_TEST_EXPECTED_DB_OID")
        .unwrap()
        .parse()
        .unwrap();
    let expected_system = std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").unwrap();
    let port: u16 = std::env::var("TECT_TEST_EXPECTED_PG_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    for (url, user) in [(&admin_url, "postgres"), (&runtime_url, "tect_ci")] {
        let parsed = PgConnectOptions::from_str(url).unwrap();
        assert_eq!(parsed.get_host(), "127.0.0.1");
        assert_eq!(parsed.get_port(), port);
        assert_eq!(parsed.get_username(), user);
        assert_eq!(parsed.get_database(), Some(name.as_str()));
        assert!(parsed.get_socket().is_none());
    }
    let pool = PgPool::connect(&admin_url).await.unwrap();
    let identity: (i32,String,i64,String) = sqlx::query_as("SELECT current_setting('server_version_num')::integer,current_database(),(SELECT oid::bigint FROM pg_database WHERE datname=current_database()),(SELECT system_identifier::text FROM pg_control_system())")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(identity, (180006, name, expected_oid, expected_system));
    let residue: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM pg_namespace WHERE nspname NOT IN ('pg_catalog','pg_toast','public','information_schema')) + (SELECT count(*) FROM pg_class WHERE relnamespace='public'::regnamespace) + (SELECT count(*) FROM pg_proc WHERE pronamespace='public'::regnamespace) + (SELECT count(*) FROM pg_extension WHERE extname <> 'plpgsql') + (SELECT count(*) FROM pg_event_trigger)")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(residue, 0, "S05 database must be new and empty");
    assert_eq!(
        std::env::var("TECT_TEST_RUNTIME_ROLE").as_deref(),
        Ok("tect_ci")
    );
    let runtime = PgPool::connect(&runtime_url).await.unwrap();
    let runtime_user: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&runtime)
        .await
        .unwrap();
    assert_eq!(runtime_user, "tect_ci");
    runtime.close().await;
    admin::migrate(&pool, "tect_ci").await.unwrap();
    (pool, runtime_url)
}

async fn signed_s05_budget(
    store: &PgStore,
    enrolled: &tect_postgres::admin::Enrollment,
    workspace_key: &str,
) -> (Uuid, BudgetOwnerKeys) {
    let keypair = Ed25519KeyPair::from_seed_unchecked(&[92u8; 32]).unwrap();
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
        provider_calls: 2, // one synthetic Matrix, one real route adviser
        input_tokens: 24_000,
        output_tokens: 2_000,
        request_utf8_bytes: MAX_REQUEST as i64,
        elapsed_monotonic_ms: 60_000,
        retry_dispatches: 1, // schema floor; the harness has no retry path
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
        &json!([{"workspace_id":workspace,
        "owner_id":enrolled.principal_id,"public_key_hex":hex(keypair.public_key().as_ref())}])
        .to_string(),
    )
    .unwrap();
    (workspace, keys)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit S05_ONE_SHOT_MODE=preflight or send; fresh owned PostgreSQL 18 only"]
async fn s05_real_adviser_one_shot() {
    let mode = std::env::var("S05_ONE_SHOT_MODE").expect("set preflight or send");
    assert!(matches!(mode.as_str(), "preflight" | "send" | "loopback"));
    let call_id = call_id();
    if mode == "send" {
        assert!(
            std::env::var("S05_ONE_SHOT_CALL_ID").is_ok(),
            "send requires explicit unique call ID"
        );
    }
    let request_path = artifact("request.json");
    let manifest_path = artifact("manifest.json");
    let marker = artifact("used");
    if mode != "loopback" {
        assert!(
            !request_path.exists() && !manifest_path.exists() && !marker.exists(),
            "existing one-shot artifact; no send"
        );
    }
    let (pool, runtime_url) = fresh_database().await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("s05-live.sock");
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let workspace_key = format!("s05-live-{}", Uuid::new_v4());
    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
    // One local synthetic Matrix call and at most one real route-adviser call.
    let (workspace, owner_keys) = signed_s05_budget(&store, &enrolled, &workspace_key).await;
    let limits: (i64,i64,i64,i64,i64,i64) = sqlx::query_as("SELECT provider_calls,input_tokens,output_tokens,request_utf8_bytes,elapsed_monotonic_ms,retry_dispatches FROM advisory_budget_policies WHERE workspace_id=$1")
        .bind(workspace).fetch_one(&pool).await.unwrap();
    assert_eq!(limits, (2, 24_000, 2_000, MAX_REQUEST as i64, 60_000, 1));
    let trusted = store.clone().with_budget_owner_keys(owner_keys.clone());
    let mut verification = trusted.begin(TransactionMode::ReadOnly).await.unwrap();
    verification.authenticate(&enrolled.auth).await.unwrap();
    verification.set_tenant(enrolled.tenant_id).await.unwrap();
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let accepted = verification
        .advisory_budget_policy_store()
        .unwrap()
        .authorized_budget_policy(workspace, now)
        .await
        .unwrap()
        .expect("signed S05 budget not trusted");
    assert_eq!(accepted.ceilings().provider_calls, 2);
    assert_eq!(accepted.ceilings().request_utf8_bytes, MAX_REQUEST as i64);
    verification.commit().await.unwrap();
    let matrix_calls = Arc::new(AtomicUsize::new(0));
    let loopback_listener = if mode == "loopback" {
        Some(TcpListener::bind("127.0.0.1:0").await.unwrap())
    } else {
        None
    };
    let loopback_endpoint = loopback_listener
        .as_ref()
        .map(|listener| format!("http://{}/v1/systemone", listener.local_addr().unwrap()));
    let ranking_provider: Arc<dyn ModelRouteRankingProvider> =
        if let Some(endpoint) = &loopback_endpoint {
            Arc::new(provider_at(endpoint, "s05-loopback-only".into()))
        } else {
            Arc::new(FakeJev {
                calls: Arc::new(AtomicUsize::new(0)),
            })
        };
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(store.clone().with_budget_owner_keys(owner_keys.clone())),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(Evidence))
        .with_matrix_advisory_adapters(
            Arc::new(Provider(matrix_calls.clone())),
            Arc::new(Budget(Arc::new(AtomicUsize::new(0)))),
        )
        .with_model_route_catalogue_provider(Arc::new(Routes))
        .with_model_route_host_capabilities_provider(Arc::new(HostCapabilities))
        .with_model_route_ranking_provider(ranking_provider),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    let owner_file = root.join("owner.json");
    host_file(&owner_file, &enrolled.auth);
    let native = Uuid::new_v4().to_string();
    let mut owner = Mcp::start(&socket, &owner_file, &native, &workspace_key).await;
    let (source, candidate) = ready_source_candidate(&mut owner, &repo).await;
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
    propose_confirm(&mut owner, program, 0, declarations("demo")).await;
    let effective = route(
        &mut owner,
        "query",
        "engineering.matrix.context.effective.get",
        json!({"locator":locator(program)}),
    )
    .await;
    let task = Uuid::new_v4();
    let recorded = record(&mut owner, task, Uuid::new_v4(), Some(locator(program))).await;
    route(&mut owner,"command","workspace.advisory.configure",json!({"expected_revision":0,"mode":"optional","provider_profile_ref":{"id":PROFILE},"model_configuration":{"model":MODEL}})).await;
    let verifier = admin::prepare_verifier_enrollment(&pool, enrolled.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let verifier_file = root.join("verifier.json");
    host_file(&verifier_file, &verifier.auth);
    let mut independent = Mcp::start(
        &socket,
        &verifier_file,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    independent.call("open_workspace", json!({})).await;
    let matrix: EngineeringMatrixInput = serde_json::from_value(recorded["input"].clone()).unwrap();
    let context: EffectiveMatrixRequirements = serde_json::from_value(effective).unwrap();
    let facts = required_matrix_operating_facts(&context, &matrix).unwrap();
    let evidence: Vec<_> = facts.iter().map(|fact|json!({"fact_path":fact.path,"evidence_ref":format!("urn:fixture:s05-live:{}",fact.path)})).collect();
    let verified = route(&mut independent,"command","engineering.matrix.verify",json!({"task_id":task,"expected_revision":1,"input_digest":recorded["input_digest"],"evidence":evidence})).await;
    let advice_key = format!("s05-live-matrix-{}", Uuid::new_v4());
    let advised = advice(&mut owner, task, &advice_key).await;
    assert_eq!(advised["state"], "advised");
    let current = route(
        &mut owner,
        "query",
        "engineering.advisory.get",
        json!({"task_id":task,"request_key":advice_key}),
    )
    .await;
    let chosen = route(&mut owner,"command","engineering.matrix.disposition.record",json!({"request_id":Uuid::new_v4(),"task_id":task,"expected_task_revision":1,"expected_input_digest":recorded["input_digest"],"expected_choice_set_digest":recorded["choice_set_digest"],"opportunity_id":advised["opportunity_id"],"basis":"after_advice","advice_id":current["current_advice"]["advice_id"],"advice_digest":current["current_advice"]["advice_digest"],"decision":{"outcome":"selected","selected_choice_id":"b"}})).await;
    let selection = json!({"task_id":task,"task_revision":1,"disposition_id":chosen["disposition_id"],"selected_choice_id":"b","expected_input_digest":recorded["input_digest"],"expected_choice_set_digest":recorded["choice_set_digest"],"expected_verification_digest":verified["verification_digest"],"mapped_draft_node_indices":[0]});
    let opened = route(&mut owner,"command","scope.open",json!({"request_id":Uuid::new_v4(),"candidate_set_id":source["candidate_set"]["id"],"candidate_set_revision":source["candidate_set"]["revision"],"candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],"candidate_revision":candidate["revision"]})).await;
    let mut save = save_request(&opened["created"]["planning"], selection);
    save["draft"]["nodes"][0]["model_route_facts"] = json!({"role":"agent","tool":"code","data_class":"internal","remaining_budget_units":20,"available_latency_ms":100});
    let caller_request = save["request_id"].clone();
    let saved = route(&mut owner, "command", "slice.candidates.save", save).await;
    let set = saved["candidate_set"]["id"].clone();
    let work = saved["draft"]["nodes"][0].clone();
    let effect = route(
        &mut independent,
        "query",
        "engineering.matrix.planning_effect.get",
        json!({"candidate_set_id":set,"caller_request_id":caller_request}),
    )
    .await;
    route(&mut independent,"command","engineering.matrix.planning_effect.verify",json!({"request_id":Uuid::new_v4(),"candidate_set_id":set,"caller_request_id":caller_request,"expected_result_revision":effect["material"]["result_revision"],"expected_effect_digest":effect["effect_digest"],"verdict":"matches","summary":"Synthetic Work maps selected Matrix choice b."})).await;
    route(&mut owner,"command","workspace.advisory.configure",json!({"expected_revision":1,"mode":"optional","provider_profile_ref":{"id":call_id},"model_configuration":{"model":ADVISER}})).await;
    let key = format!("s05-live-route-{}", Uuid::new_v4());
    let prepared = route(&mut owner,"command","model.route.prepare",json!({"disposition_id":chosen["disposition_id"],"expected_task_id":task,"expected_task_revision":1,"expected_candidate_set_id":set,"expected_caller_request_id":caller_request,"expected_mapped_work_node_id":work["id"],"expected_mapped_work_node_revision":work["revision"],"request_key":key,"requested_route_id":"route-a"})).await;
    assert_eq!(prepared["preparation"], "Prepared");
    assert_eq!(prepared["routes"]["requested_route_id"], "route-a");
    assert!(prepared["routes"]["observed_actual"].is_null());
    let typed: PreparedModelRouteRecommendation = serde_json::from_value(prepared.clone()).unwrap();
    if mode == "loopback" {
        loopback::run(
            &pool,
            loopback_listener.unwrap(),
            loopback_endpoint.unwrap(),
            &mut owner,
            &socket,
            &owner_file,
            &workspace_key,
            &key,
            workspace,
            id(&set),
            id(&caller_request),
            typed,
        )
        .await;
        owner.finish().await;
        independent.finish().await;
        server.abort();
        let _ = server.await;
        return;
    }
    let exact = provider("preflight-placeholder-never-sent".into())
        .prepare(&typed)
        .unwrap()
        .request_bytes;
    assert!(!exact.is_empty() && exact.len() < MAX_REQUEST);
    let digest = format!("{:x}", Sha256::digest(&exact));
    let wire: Value = serde_json::from_slice(&exact).unwrap();
    assert_eq!(wire["model"], ADVISER);
    let binding = serde_json::to_vec(&(
        ENDPOINT,
        call_id.as_str(),
        ADVISER,
        "tect.model-route-typesafe-choice/1",
    ))
    .unwrap();
    assert_eq!(
        wire["state"]["provider_binding_digest"],
        format!("{:x}", Sha256::digest(binding))
    );
    let routes = wire["state"]["request"]["eligible_routes"]
        .as_array()
        .unwrap();
    assert_eq!(routes.len(), 2);
    assert!(
        routes
            .iter()
            .all(|route| route["provider"].as_str().unwrap().ends_with(".invalid"))
    );
    assert!(routes.iter().all(|route| route["model"] != ADVISER));
    let manifest = json!({"call_id":call_id,"endpoint":ENDPOINT,"adviser_model":ADVISER,"request_bytes":exact.len(),"request_sha256":digest,"workspace_id":workspace,"task_id":task,"candidate_set_id":set,"work_node_id":work["id"],"preparation_request_key":key,"requested_route_id":"route-a","candidate_routes":routes,"budget":{"provider_calls_total":2,"matrix_synthetic_calls":1,"route_real_calls_max":1,"input_tokens":24000,"output_tokens":2000,"request_utf8_bytes_max":MAX_REQUEST,"timeout_ms":10000,"retry_dispatches_policy_ceiling":1,"harness_retries":0}});
    exclusive(&request_path, &exact);
    exclusive(
        &manifest_path,
        &serde_json::to_vec_pretty(&manifest).unwrap(),
    );
    println!(
        "S05 preflight bytes={} sha256={digest} request={} manifest={}",
        exact.len(),
        request_path.display(),
        manifest_path.display()
    );
    owner.finish().await;
    independent.finish().await;
    server.abort();
    let _ = server.await;
    if mode == "preflight" {
        assert!(!marker.exists());
        let attempts: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM model_route_advisory_attempts WHERE workspace_id=$1",
        )
        .bind(workspace)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(attempts, 0);
        return;
    }
    eprintln!(
        "Review {} and {}. Enter exactly: SEND JEV {digest}",
        request_path.display(),
        manifest_path.display()
    );
    assert!(
        confirmed(&mut std::io::stdin().lock(), &digest),
        "confirmation mismatch; no send"
    );
    let credential = std::env::var("TYPESAFE_API_KEY").expect("send needs private process key");
    assert!(!credential.trim().is_empty());
    assert_eq!(
        provider(credential.clone())
            .prepare(&typed)
            .unwrap()
            .request_bytes,
        exact
    );
    exclusive(
        &marker,
        format!("call_id={call_id}\nrequest_sha256={digest}\n").as_bytes(),
    );
    let send_socket = root.join("s05-send.sock");
    let send_service = Arc::new(
        WorkspaceService::new(
            Arc::new(store.with_budget_owner_keys(owner_keys)),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_model_route_catalogue_provider(Arc::new(Routes))
        .with_model_route_host_capabilities_provider(Arc::new(HostCapabilities))
        .with_model_route_ranking_provider(Arc::new(ReviewedProvider {
            inner: provider(credential),
            reviewed: exact.clone(),
            marker: marker.clone(),
        })),
    );
    let listener = UnixListener::bind(&send_socket).unwrap();
    fs::set_permissions(&send_socket, fs::Permissions::from_mode(0o600)).unwrap();
    let send_server = tokio::spawn(tect_host::serve(listener, send_service));
    let mut owner = Mcp::start(&send_socket, &owner_file, &native, &workspace_key).await;
    owner.call("open_workspace", json!({})).await;
    let raw_run = owner
        .exchange(
            "tools/call",
            recovery_support::public_call(
                "command",
                json!({"route":"model.route.run","params":{"preparation_request_key":key}}),
            ),
        )
        .await;
    let audit: (Vec<u8>,String,Option<Vec<u8>>,Option<String>,String) = sqlx::query_as("SELECT request_payload,request_sha256,response_payload,response_sha256,state FROM model_route_advisory_attempts WHERE workspace_id=$1 AND preparation_request_key=$2")
        .bind(workspace).bind(&key).fetch_one(&pool).await.unwrap();
    assert_eq!(audit.0, exact);
    assert_eq!(audit.1, digest);
    if let Some(raw) = &audit.2 {
        assert_eq!(audit.3, Some(format!("{:x}", Sha256::digest(raw))));
    }
    if raw_run["result"]["isError"] == true || !raw_run["error"].is_null() {
        assert!(matches!(
            audit.4.as_str(),
            "send_unknown" | "raw_sealed" | "parsed"
        ));
        let observer = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
            .await
            .unwrap();
        let counts: (i64,i64,i64,i64,i64,i64) = sqlx::query_as("SELECT (SELECT coalesce(sum(call_count),0)::bigint FROM advisory_call_audit WHERE workspace_id=$1 AND capability='model_routing'),(SELECT count(*) FROM model_route_advisory_attempts WHERE workspace_id=$1),(SELECT count(*) FROM model_route_budget_reservations WHERE workspace_id=$1),(SELECT count(*) FROM model_route_budget_consumptions WHERE workspace_id=$1),(SELECT count(*) FROM model_route_decisions WHERE workspace_id=$1),(SELECT count(*) FROM model_route_dispositions WHERE workspace_id=$1)")
            .bind(workspace).fetch_one(&observer).await.unwrap();
        assert_eq!(
            (counts.0, counts.1, counts.2, counts.4, counts.5),
            (1, 1, 1, 0, 0)
        );
        let transport:(Option<bool>,Option<Value>,Option<i32>) = sqlx::query_as("SELECT response_complete,original_transport_context,response_http_status FROM model_route_advisory_attempts WHERE workspace_id=$1 AND preparation_request_key=$2")
            .bind(workspace).bind(&key).fetch_one(&observer).await.unwrap();
        if audit.2.is_some() {
            assert_eq!(counts.3, 1);
            assert!(transport.0.is_some() && transport.1.is_some() && transport.2.is_some());
            let context = transport.1.as_ref().unwrap();
            assert_eq!(context["send_certainty"], "sent");
            assert_eq!(
                context["raw_response_ref"],
                format!("sha256:{}", audit.3.as_ref().unwrap())
            );
        } else {
            assert_eq!(audit.4, "send_unknown");
            assert_eq!(counts.3, 0);
            assert_eq!(transport, (None, None, None));
        }
        observer.close().await;
        let error = if raw_run["error"].is_null() {
            recovery_support::tool_payload(&raw_run)
        } else {
            raw_run["error"].clone()
        };
        println!(
            "S05 provider error retained audit_state={} response_bytes={} response_sha256={:?} error={error}",
            audit.4,
            audit.2.as_ref().map_or(0, Vec::len),
            audit.3
        );
        owner.finish().await;
        send_server.abort();
        let _ = send_server.await;
        return; // Marker remains spent; uncertain send is never retried.
    }
    let run = recovery_support::tool_payload(&raw_run);
    let replay = route(
        &mut owner,
        "command",
        "model.route.run",
        json!({"preparation_request_key":key}),
    )
    .await;
    assert_eq!(run, replay);
    if run["decision"].is_null() {
        assert_eq!(run["attempt"]["state"], "budget_exhausted", "{run}");
        let observer = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
            .await
            .unwrap();
        let counts:(i64,i64,i64,i64) = sqlx::query_as("SELECT (SELECT coalesce(sum(call_count),0)::bigint FROM advisory_call_audit WHERE workspace_id=$1 AND capability='model_routing'),(SELECT count(*) FROM model_route_budget_consumptions WHERE workspace_id=$1),(SELECT count(*) FROM model_route_decisions WHERE workspace_id=$1),(SELECT count(*) FROM model_route_dispositions WHERE workspace_id=$1)")
            .bind(workspace).fetch_one(&observer).await.unwrap();
        assert_eq!(counts, (1, 1, 0, 0));
        observer.close().await;
        println!(
            "S05 budget stop audit_state={} response_sha256={:?} actual=null",
            audit.4, audit.3
        );
        owner.finish().await;
        send_server.abort();
        let _ = send_server.await;
        return;
    }
    assert_eq!(run["decision"]["routes"]["requested_route_id"], "route-a");
    assert!(run["decision"]["routes"]["observed_actual"].is_null());
    if let Some(recommended) = run["decision"]["routes"]["recommended_route_id"].as_str() {
        assert!(recommended == "route-a" || recommended == "route-b");
        let before = route(
            &mut owner,
            "query",
            "model.route.get",
            json!({"preparation_request_key":key}),
        )
        .await;
        assert_eq!(before["decision"]["id"], run["decision"]["id"]);
        let source_session: Uuid = sqlx::query_scalar("SELECT caller_session_id FROM matrix_planning_selection_links WHERE workspace_id=$1 AND candidate_set_id=$2 AND caller_request_id=$3")
            .bind(workspace).bind(id(&set)).bind(id(&caller_request)).fetch_one(&pool).await.unwrap();
        admin::revoke_session(&pool, source_session).await.unwrap();
        let mut fresh = Mcp::start(
            &send_socket,
            &owner_file,
            &Uuid::new_v4().to_string(),
            &workspace_key,
        )
        .await;
        fresh.call("open_workspace", json!({})).await;
        let disposition = route(&mut fresh,"command","model.route.disposition",json!({"disposition_id":Uuid::new_v4(),"decision_id":run["decision"]["id"],"action":"accept","rationale":"Fresh owner accepts adviser recommendation without model execution"})).await;
        assert_eq!(disposition["action"], "Accept");
        let saved:(Uuid,Uuid,String,Value) = sqlx::query_as("SELECT d.id,d.decision_id,d.action,x.decision_payload FROM model_route_dispositions d JOIN model_route_decisions x ON (x.tenant_id,x.workspace_id,x.id)=(d.tenant_id,d.workspace_id,d.decision_id) WHERE d.workspace_id=$1 AND d.id=$2")
            .bind(workspace).bind(id(&disposition["id"])).fetch_one(&pool).await.unwrap();
        assert_eq!(
            (saved.0, saved.1, saved.2),
            (
                id(&disposition["id"]),
                id(&run["decision"]["id"]),
                "accept".into()
            )
        );
        assert!(saved.3["routes"]["observed_actual"].is_null());
        fresh.finish().await;
    } else {
        assert!(run["decision"]["outcome"]["Abstained"].is_object(), "{run}");
        assert!(run["decision"]["routes"]["recommended_route_id"].is_null());
        assert_eq!(run["decision"]["routes"]["requested_route_id"], "route-a");
        assert!(run["decision"]["routes"]["observed_actual"].is_null());
        let dispositions: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM model_route_dispositions WHERE workspace_id=$1",
        )
        .bind(workspace)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(dispositions, 0);
    }
    let observed: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM model_route_advisory_attempts WHERE workspace_id=$1),(SELECT count(*) FROM model_route_budget_reservations WHERE workspace_id=$1),(SELECT count(*) FROM model_route_dispositions WHERE workspace_id=$1)")
        .bind(workspace).fetch_one(&pool).await.unwrap();
    assert_eq!(observed.0, 1);
    assert_eq!(observed.1, 1);
    assert!(observed.2 <= 1);
    // Separate read-only administrator connection observes the durable call
    // ledger after the caller has completed. This is independent of MCP get.
    let observer = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    let audit_counts: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT coalesce(sum(call_count),0)::bigint FROM advisory_call_audit WHERE workspace_id=$1 AND capability='model_routing'),(SELECT count(*) FROM model_route_budget_consumptions WHERE workspace_id=$1),(SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1)")
        .bind(workspace).fetch_one(&observer).await.unwrap();
    assert_eq!(
        audit_counts,
        (1, 1, 1),
        "one route adviser call and one synthetic Matrix call only"
    );
    let immutable: (String,Option<bool>,Option<Value>,Option<i32>) = sqlx::query_as("SELECT adapter_identity,response_complete,original_transport_context,response_http_status FROM model_route_advisory_attempts WHERE workspace_id=$1 AND preparation_request_key=$2")
        .bind(workspace).bind(&key).fetch_one(&observer).await.unwrap();
    assert_eq!(immutable.0, "tect.model-route-typesafe-choice/1");
    if audit.2.is_some() {
        assert!(immutable.1.is_some());
        assert!(immutable.2.is_some());
        assert!(immutable.3.is_some());
    }
    observer.close().await;
    println!(
        "S05 result={} audit_state={} response_bytes={} response_sha256={:?} requested=route-a recommended={} actual=null",
        run["decision"]["outcome"],
        audit.4,
        audit.2.as_ref().map_or(0, Vec::len),
        audit.3,
        run["decision"]["routes"]["recommended_route_id"]
    );
    owner.finish().await;
    send_server.abort();
    let _ = send_server.await;
}
