//! S01 public/native MCP regression on the owned, disposable ledger-110 PG18 fixture.
//! The provider is synthetic; this test never contacts JEV or migrates the database.
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use async_trait::async_trait;
use recovery_support::{Mcp, host_file, private_temp, tagged_url};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};
use sha2::{Digest, Sha384};
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
    PreparedScopeAdviceAttempt, ScopeAdviceProvider, ScopeAdviceProviderContext,
    ScopeAdviceProviderError, ScopeAdviceProviderObservation, ScopeAdviceProviderRequest,
    ScopeAuthorityObserver, ScopeAuthorityOutcome, ScopeAuthorityRequest, ScopeBudgetPolicy,
    ScopeBudgetPolicyEvaluation, ScopeBudgetRequest, StartedScopeDispatchPermit, Store,
    TransactionMode, WorkspaceService,
};
use tect_domain::{
    AdvisoryBudgetCeilings, AdvisoryBudgetPolicy, AdvisoryDispatchOutcome, AdvisorySendCertainty,
    ConfidenceBasisPoints, EventKind, NormalizedScopeAdviceAnswer, NormalizedScopeAdviceAnswers,
    ScopeAdviceChoice, ScopeAdviceScoreBand,
};
use tect_postgres::{
    BudgetOwnerKeys, PgScopeAuthoredManifestSupplier, PgScopeAuthorityObserver, PgStore, admin,
};
use tokio::net::UnixListener;
use uuid::Uuid;

const SYSTEM_ID: &str = "7689676854994613066";
const DATABASE_OID: i64 = 16385;

async fn owned_ledger110() -> (PgPool, String) {
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
        "SELECT current_setting('server_version_num')::integer,current_database(),current_user, \
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()), \
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
    let runtime_pool = PgPool::connect_with(runtime_options).await.unwrap();
    let runtime_identity: (String, String, i64) = sqlx::query_as(
        "SELECT current_database(),current_user, \
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database())",
    )
    .fetch_one(&runtime_pool)
    .await
    .unwrap();
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
    assert_eq!(
        ledger[109].2,
        Sha384::digest(include_bytes!(
            "../../postgres/migrations/0110_pipeline_context_matrix_authority.sql"
        ))
        .to_vec(),
        "fixture migration 110 differs from reviewed source"
    );
    (pool, runtime_url)
}

struct SyntheticBudget;

#[async_trait]
impl ScopeBudgetPolicy for SyntheticBudget {
    async fn evaluate(
        &self,
        _: &ScopeBudgetRequest,
        policy: &tect_domain::AdvisoryBudgetPolicy,
    ) -> tect_domain::Result<Option<ScopeBudgetPolicyEvaluation>> {
        Ok(Some(ScopeBudgetPolicyEvaluation {
            policy_id: policy.id().to_string(),
            policy_version: policy.version(),
            policy_digest: policy.digest().to_owned(),
        }))
    }
}

struct FakeJev {
    calls: Arc<AtomicUsize>,
    pause: Option<Arc<PauseProviderAttempt>>,
}

struct PauseProviderAttempt {
    arm: tokio::sync::Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
}

impl PauseProviderAttempt {
    fn new() -> Self {
        Self {
            arm: tokio::sync::Mutex::new(None),
        }
    }

    async fn arm(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (reached_tx, reached_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        *self.arm.lock().await = Some((reached_tx, release_rx));
        (reached_rx, release_tx)
    }
}

struct PauseSecondObservation {
    inner: Arc<dyn ScopeAuthorityObserver>,
    arm: tokio::sync::Mutex<Option<ObservationArm>>,
}

struct ObservationArm {
    candidate: Uuid,
    seen: usize,
    reached: Option<tokio::sync::oneshot::Sender<()>>,
    release: Option<tokio::sync::oneshot::Receiver<()>>,
}

impl PauseSecondObservation {
    fn new(inner: Arc<dyn ScopeAuthorityObserver>) -> Self {
        Self {
            inner,
            arm: tokio::sync::Mutex::new(None),
        }
    }

    async fn arm(
        &self,
        candidate: Uuid,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (reached_tx, reached_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        *self.arm.lock().await = Some(ObservationArm {
            candidate,
            seen: 0,
            reached: Some(reached_tx),
            release: Some(release_rx),
        });
        (reached_rx, release_tx)
    }
}

#[async_trait]
impl ScopeAuthorityObserver for PauseSecondObservation {
    async fn observe(
        &self,
        request: &ScopeAuthorityRequest,
    ) -> tect_domain::Result<ScopeAuthorityOutcome> {
        let pause = {
            let mut guard = self.arm.lock().await;
            if let Some(arm) = guard
                .as_mut()
                .filter(|arm| arm.candidate == request.candidate_set_id)
            {
                arm.seen += 1;
                if arm.seen == 2 {
                    let reached = arm.reached.take();
                    let release = arm.release.take();
                    *guard = None;
                    reached.zip(release)
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some((reached, release)) = pause {
            let _ = reached.send(());
            release
                .await
                .map_err(|_| tect_domain::Error::TransportUnavailable)?;
        }
        self.inner.observe(request).await
    }
}

#[async_trait]
impl ScopeAdviceProvider for FakeJev {
    fn identity(&self) -> Option<(&'static str, &'static str)> {
        Some(("fixture", "v1"))
    }

    fn prepare_context(
        &self,
        context: &ScopeAdviceProviderContext,
    ) -> std::result::Result<PreparedScopeAdviceAttempt, ScopeAdviceProviderError> {
        let request = context.request();
        PreparedScopeAdviceAttempt::new(
            request.clone(),
            serde_json::to_vec(request).unwrap(),
            "fixture-profile".into(),
            "fixture-model".into(),
            "https://fixture.invalid/advice".into(),
            "fixture-wire/1".into(),
        )
    }

    async fn attempt_prepared(
        &self,
        request: &ScopeAdviceProviderRequest,
        prepared: PreparedScopeAdviceAttempt,
        permit: StartedScopeDispatchPermit,
    ) -> std::result::Result<ScopeAdviceProviderObservation, ScopeAdviceProviderError> {
        assert_eq!(prepared.request(), &request.request);
        assert!(permit.permits(request.dispatch_id, &prepared));
        assert_eq!(request.request.alternatives.len(), 2);
        if let Some(pause) = &self.pause
            && let Some((reached, release)) = pause.arm.lock().await.take()
        {
            let _ = reached.send(());
            release.await.expect("provider pause released");
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        let answers = request
            .request
            .alternatives
            .iter()
            .map(|alternative| NormalizedScopeAdviceAnswer {
                alternative_id: alternative.id.clone(),
                choice: if alternative.id == request.request.baseline_id {
                    ScopeAdviceChoice::Preferred
                } else {
                    ScopeAdviceChoice::NonPreferred
                },
                score: if alternative.id == request.request.baseline_id {
                    ScopeAdviceScoreBand::StrongFit
                } else {
                    ScopeAdviceScoreBand::WeakFit
                },
                choice_confidence: ConfidenceBasisPoints(8_000),
                score_confidence: ConfidenceBasisPoints(7_000),
            })
            .collect();
        Ok(ScopeAdviceProviderObservation {
            send_certainty: AdvisorySendCertainty::Sent,
            outcome: AdvisoryDispatchOutcome::ProviderResponse,
            answers: Some(NormalizedScopeAdviceAnswers { answers }),
            response_payload: Some(b"synthetic-provider-response".to_vec()),
            input_tokens: Some(11),
            output_tokens: Some(5),
            latency_ms: Some(1),
            raw_response_ref: Some("fixture:scope-response".into()),
            failure_reason: None,
        })
    }
}

fn authored_draft(source_ref: Uuid, prior: &Value, title: &str) -> Value {
    json!({"boundary":"ongoing","goals":[{
        "identity":{"local":"goal"},"text":"Preserve source evidence",
        "source_ref_id":source_ref,
        "resolution":{"kind":"candidate","reference":{"local":"candidate"}}
    }],"evidence":[],"candidates":[{
        "identity":{"local":"candidate"},"title":title,
        "outcome":"The preview cause is demonstrated",
        "trigger":"Preview differs from settings",
        "delivered_behavior":"A bounded correction is selected",
        "proof":"Direct source evidence is retained",
        "includes":["diagnosis"],"excludes":["deployment"],
        "dependencies":[],"coverage_goals":[{"local":"goal"}],"evidence":[]
    }],"blockers":[],"protected_changes":[],"supersessions":[{
        "candidate_id":prior["id"],"revision":prior["revision"],
        "reason":"Compare this authored option with the prior candidate",
        "replacements":[{"local":"candidate"}]
    }]})
}

async fn authored_scope_set(
    client: &mut Mcp,
    candidate_set: Uuid,
    revision: i64,
    prior: &Value,
) -> Value {
    let inputs = client
        .call(
            "candidate_context",
            json!({"candidate_set_id":candidate_set,"view":"inputs","limit":25}),
        )
        .await;
    let program = client
        .call(
            "candidate_context",
            json!({"candidate_set_id":candidate_set,"view":"program","limit":25}),
        )
        .await;
    let mut refs: Vec<Uuid> = inputs["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| id(&item["input"]["source_ref_id"]))
        .collect();
    let source_ref = *refs.first().unwrap();
    refs.extend(
        program["program"]["field_refs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| id(&field["id"])),
    );
    refs.sort_unstable();
    refs.dedup();
    json!({"expected_candidate_set_revision":revision,
        "baseline_key":"baseline","alternatives":[
            {"key":"baseline","kind":"cohesive",
             "draft":authored_draft(source_ref, prior, "Cohesive diagnosis"),
             "covered_source_ref_ids":refs},
            {"key":"partition","kind":"partitioned",
             "draft":authored_draft(source_ref, prior, "Partitioned diagnosis"),
             "covered_source_ref_ids":refs}]})
}

async fn signed_fixture_budget(
    store: &PgStore,
    enrolled: &tect_postgres::admin::Enrollment,
    workspace_key: &str,
    ceilings: AdvisoryBudgetCeilings,
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
    let hex = |bytes: &[u8]| {
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let signed = AdvisoryBudgetPolicy::new(
        id,
        1,
        digest,
        from,
        until,
        ceilings,
        enrolled.principal_id,
        hex(keypair
            .sign(&unsigned.approval_signing_message(workspace).unwrap())
            .as_ref()),
    )
    .unwrap();
    tx.advisory_budget_policy_store()
        .unwrap()
        .install_budget_policy(workspace, &signed)
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

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires fresh disposable PostgreSQL 18 and TECT_TEST_*; local synthetic provider only"]
async fn public_scope_session_preference_is_bound_snapshotted_and_replay_stable() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let pool = PgPool::connect(&admin_url).await.unwrap();
    let version: i32 = sqlx::query_scalar("SELECT current_setting('server_version_num')::integer")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!((180000..190000).contains(&version));
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("scope-session-preference.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("scope-session-pref-{}", Uuid::new_v4()),
    );
    let store = PgStore::connect(&runtime, 4).await.unwrap();
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let workspace_key = format!("scope-session-pref-{}", Uuid::new_v4());
    let (workspace, keys) = signed_fixture_budget(
        &store,
        &enrollment,
        &workspace_key,
        AdvisoryBudgetCeilings {
            provider_calls: 3,
            input_tokens: 1_000,
            output_tokens: 1_000,
            request_utf8_bytes: 2_000_000,
            elapsed_monotonic_ms: 120_000,
            retry_dispatches: 1,
        },
    )
    .await;
    let trusted = store.with_budget_owner_keys(keys);
    let authority = Arc::new(PgScopeAuthorityObserver::new(
        trusted.clone(),
        Arc::new(tect_host::StaticCandidateGuidance),
    ));
    let supplier = Arc::new(PgScopeAuthoredManifestSupplier::new(
        trusted.clone(),
        authority.clone(),
    ));
    let paused_authority = Arc::new(PauseSecondObservation::new(authority));
    let calls = Arc::new(AtomicUsize::new(0));
    let provider_pause = Arc::new(PauseProviderAttempt::new());
    let service = Arc::new(WorkspaceService::new_with_scope_advisory_adapters(
        Arc::new(trusted),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
        paused_authority.clone(),
        supplier,
        Arc::new(SyntheticBudget),
        Arc::new(FakeJev {
            calls: calls.clone(),
            pause: Some(provider_pause.clone()),
        }),
    ));
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service.clone()));
    let host = root.join("host.json");
    host_file(&host, &enrollment.auth);
    let session_a = Uuid::new_v4().to_string();
    let session_b = Uuid::new_v4().to_string();
    let mut a = Mcp::start(&socket, &host, &session_a, &workspace_key).await;
    let mut b = Mcp::start(&socket, &host, &session_b, &workspace_key).await;
    b.call("open_workspace", json!({})).await;
    let (source, prior) = ready_source_candidate(&mut a, &repo).await;
    // The candidate snapshot binds the selected source set. Mirror A's sole
    // registered worktree into B's independent native-session selection so
    // B can observe the same source fingerprint while retaining its own
    // default advisory preference.
    let sources = b.call("list_sources", json!({"limit":25})).await;
    let source_items = sources["items"].as_array().unwrap();
    assert_eq!(source_items.len(), 1);
    b.call(
        "select_worktrees",
        json!({"worktree_ids":[source_items[0]["id"]]}),
    )
    .await;
    let candidate = id(&source["candidate_set"]["id"]);
    let revision = source["candidate_set"]["revision"].as_i64().unwrap();
    let configured = route(
        &mut a,
        "command",
        "workspace.advisory.configure",
        json!({"expected_revision":0,"mode":"optional",
            "provider_profile_ref":{"id":"fixture-profile"},
            "model_configuration":{"model":"fixture-model"}}),
    )
    .await;
    assert_eq!(id(&configured["workspace_id"]), workspace);
    let skip = route(
        &mut a,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":0,"preference":"skip"}),
    )
    .await;
    assert_eq!(skip["revision"], 1);
    let skipped_params = json!({"request_id":Uuid::new_v4(),"candidate_set_id":candidate});
    let skipped = route(
        &mut a,
        "command",
        "scope.advisory.request",
        skipped_params.clone(),
    )
    .await;
    assert_eq!(skipped["state"], "no_call");
    assert_eq!(skipped["reason"], "session_skip");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let skipped_id = id(&skipped["opportunity_id"]);
    let skipped_detail = route(
        &mut a,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":candidate,"opportunity_id":skipped_id}),
    )
    .await;
    assert_eq!(skipped_detail["opportunity"]["session_preference"], "skip");
    assert!(skipped_detail["dispatches"].as_array().unwrap().is_empty());

    let authored = authored_scope_set(&mut a, candidate, revision, &prior).await;
    let b_params = json!({"request_id":Uuid::new_v4(),"candidate_set_id":candidate,
        "authored_scope_set":authored});
    let b_result = route(&mut b, "command", "scope.advisory.request", b_params).await;
    assert_eq!(b_result["state"], "advised", "{b_result}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let b_detail = route(
        &mut b,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":candidate,"opportunity_id":b_result["opportunity_id"]}),
    )
    .await;
    assert_eq!(
        b_detail["opportunity"]["session_preference"],
        "use_workspace"
    );
    assert_eq!(b_detail["dispatches"].as_array().unwrap().len(), 1);

    let use_workspace = route(
        &mut a,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":1,"preference":"use_workspace"}),
    )
    .await;
    assert_eq!(use_workspace["revision"], 2);
    let replay = route(&mut a, "command", "scope.advisory.request", skipped_params).await;
    assert_eq!(replay["opportunity_id"], skipped["opportunity_id"]);
    assert_eq!(replay["reason"], "session_skip");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let forged = route_error(
        &mut a,
        "command",
        "scope.advisory.request",
        json!({"request_id":Uuid::new_v4(),"candidate_set_id":candidate,
            "session_preference":"skip"}),
    )
    .await;
    assert_eq!(forged["error"]["code"], "invalid_arguments");
    let request_skip = route(
        &mut a,
        "command",
        "scope.advisory.request",
        json!({"request_id":Uuid::new_v4(),"candidate_set_id":candidate,
            "request_preference":"skip"}),
    )
    .await;
    assert_eq!(request_skip["reason"], "request_skip");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // The anti-bloat binding is intentionally unique per candidate-set
    // revision. Exercise A's restored session preference on a fresh set rather
    // than trying to author a second opportunity against B's bound revision.
    let (a_source, a_prior) = ready_source_candidate(&mut a, &repo).await;
    let a_candidate = id(&a_source["candidate_set"]["id"]);
    let a_revision = a_source["candidate_set"]["revision"].as_i64().unwrap();
    let a_authored = authored_scope_set(&mut a, a_candidate, a_revision, &a_prior).await;
    let a_active_params = json!({"request_id":Uuid::new_v4(),"candidate_set_id":a_candidate,
        "authored_scope_set":a_authored});
    let a_active = route(
        &mut a,
        "command",
        "scope.advisory.request",
        a_active_params.clone(),
    )
    .await;
    assert_eq!(a_active["state"], "advised", "{a_active}");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let a_detail = route(
        &mut a,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":a_candidate,"opportunity_id":a_active["opportunity_id"]}),
    )
    .await;
    assert_eq!(
        a_detail["opportunity"]["session_preference"],
        "use_workspace"
    );
    assert_eq!(a_detail["dispatches"].as_array().unwrap().len(), 1);
    let cross_session_replay = route_error(
        &mut b,
        "command",
        "scope.advisory.request",
        a_active_params.clone(),
    )
    .await;
    assert_eq!(cross_session_replay["error"]["code"], "input_conflict");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let mut changed_request_preference = a_active_params.clone();
    changed_request_preference["request_preference"] = json!("skip");
    let changed_replay = route_error(
        &mut a,
        "command",
        "scope.advisory.request",
        changed_request_preference,
    )
    .await;
    assert_eq!(changed_replay["error"]["code"], "input_conflict");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let changed_session_preference = route(
        &mut a,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":2,"preference":"skip"}),
    )
    .await;
    assert_eq!(changed_session_preference["revision"], 3);
    let saved_replay = route(&mut a, "command", "scope.advisory.request", a_active_params).await;
    assert_eq!(saved_replay["opportunity_id"], a_active["opportunity_id"]);
    assert_eq!(saved_replay["state"], "advised");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let persisted: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT s.native_session_id,o.session_preference,o.request_preference,o.primary_reason \
         FROM advisory_opportunity o JOIN agent_sessions s \
           ON (s.tenant_id,s.workspace_id,s.id)=(o.tenant_id,o.workspace_id,o.session_id) \
         WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.id IN ($3,$4,$5,$6) ORDER BY o.created_at",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace)
    .bind(skipped_id)
    .bind(id(&b_result["opportunity_id"]))
    .bind(id(&request_skip["opportunity_id"]))
    .bind(id(&a_active["opportunity_id"]))
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(persisted.len(), 4);
    assert!(persisted.contains(&(
        session_a.clone(),
        "skip".into(),
        "use_workspace".into(),
        "session_skip".into(),
    )));
    assert!(persisted.contains(&(
        session_b.clone(),
        "use_workspace".into(),
        "use_workspace".into(),
        "provider_response".into(),
    )));
    assert!(persisted.contains(&(
        session_a.clone(),
        "use_workspace".into(),
        "skip".into(),
        "request_skip".into(),
    )));
    assert!(persisted.contains(&(
        session_a.clone(),
        "use_workspace".into(),
        "use_workspace".into(),
        "provider_response".into(),
    )));
    let skipped_dispatches: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3")
        .bind(enrollment.tenant_id).bind(workspace).bind(skipped_id)
        .fetch_one(&pool).await.unwrap();
    assert_eq!(skipped_dispatches, 0);

    // The second source observation happens after prepared material commits.
    // A committed skip during that pause must win before dispatch authorization.
    route(
        &mut a,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":3,"preference":"use_workspace"}),
    )
    .await;
    let (paused_source, paused_prior) = ready_source_candidate(&mut a, &repo).await;
    let paused_candidate = id(&paused_source["candidate_set"]["id"]);
    let paused_authored = authored_scope_set(
        &mut a,
        paused_candidate,
        paused_source["candidate_set"]["revision"].as_i64().unwrap(),
        &paused_prior,
    )
    .await;
    let paused_params = json!({"request_id":Uuid::new_v4(),
        "candidate_set_id":paused_candidate,"authored_scope_set":paused_authored});
    let (reached, release) = paused_authority.arm(paused_candidate).await;
    let mut pending = Box::pin(route(
        &mut a,
        "command",
        "scope.advisory.request",
        paused_params.clone(),
    ));
    tokio::select! {
        _ = reached => {},
        result = &mut pending => panic!("request completed before prepared pause: {result}"),
    }
    let skipped_after_prepare = service
        .set_session_advisory_preference(
            &tect_domain::RequestContext {
                auth: enrollment.auth.clone(),
                native_session_id: session_a.clone(),
                workspace_key: workspace_key.clone(),
            },
            &tect_domain::SetSessionAdvisoryPreference {
                expected_revision: 4,
                preference: tect_domain::AdvisoryRequestPreference::Skip,
            },
        )
        .await
        .unwrap();
    assert_eq!(skipped_after_prepare.revision, 5);
    release.send(()).unwrap();
    let fenced = (&mut pending).await;
    drop(pending);
    assert_eq!(fenced["state"], "no_call", "{fenced}");
    assert_eq!(fenced["reason"], "session_skip");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let fenced_detail = route(
        &mut a,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":paused_candidate,"opportunity_id":fenced["opportunity_id"]}),
    )
    .await;
    assert_eq!(
        fenced_detail["opportunity"]["session_preference"],
        "use_workspace"
    );
    assert!(fenced_detail["dispatches"].as_array().unwrap().is_empty());
    let fenced_replay = route(&mut a, "command", "scope.advisory.request", paused_params).await;
    assert_eq!(fenced_replay["opportunity_id"], fenced["opportunity_id"]);
    assert_eq!(fenced_replay["reason"], "session_skip");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        service
            .session_advisory_preference(&tect_domain::RequestContext {
                auth: enrollment.auth.clone(),
                native_session_id: session_b.clone(),
                workspace_key: workspace_key.clone(),
            })
            .await
            .unwrap()
            .preference,
        tect_domain::AdvisoryRequestPreference::UseWorkspace
    );

    // The provider seam is entered only after the one-use authorization and
    // sending transactions commit. A later skip cannot erase that attempt.
    route(
        &mut a,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":5,"preference":"use_workspace"}),
    )
    .await;
    let (authorized_source, authorized_prior) = ready_source_candidate(&mut a, &repo).await;
    let authorized_candidate = id(&authorized_source["candidate_set"]["id"]);
    let authorized_authored = authored_scope_set(
        &mut a,
        authorized_candidate,
        authorized_source["candidate_set"]["revision"]
            .as_i64()
            .unwrap(),
        &authorized_prior,
    )
    .await;
    let authorized_params = json!({"request_id":Uuid::new_v4(),
        "candidate_set_id":authorized_candidate,"authored_scope_set":authorized_authored});
    let (provider_reached, provider_release) = provider_pause.arm().await;
    let mut authorized_pending = Box::pin(route(
        &mut a,
        "command",
        "scope.advisory.request",
        authorized_params.clone(),
    ));
    tokio::select! {
        _ = provider_reached => {},
        result = &mut authorized_pending => panic!("request completed before provider pause: {result}"),
    }
    let before_skip: Vec<(String, String)> = sqlx::query_as(
        "SELECT d.state,d.send_certainty FROM advisory_dispatch d JOIN advisory_opportunity o \
         ON (o.tenant_id,o.workspace_id,o.id)=(d.tenant_id,d.workspace_id,d.opportunity_id) \
         WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.request_key=$3",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace)
    .bind(authorized_params["request_id"].as_str().unwrap())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(before_skip, vec![("sending".into(), "sent_unknown".into())]);
    service
        .set_session_advisory_preference(
            &tect_domain::RequestContext {
                auth: enrollment.auth.clone(),
                native_session_id: session_a.clone(),
                workspace_key: workspace_key.clone(),
            },
            &tect_domain::SetSessionAdvisoryPreference {
                expected_revision: 6,
                preference: tect_domain::AdvisoryRequestPreference::Skip,
            },
        )
        .await
        .unwrap();
    provider_release.send(()).unwrap();
    let authorized_result = (&mut authorized_pending).await;
    drop(authorized_pending);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let after_skip: Vec<(String, String)> = sqlx::query_as(
        "SELECT d.state,d.send_certainty FROM advisory_dispatch d JOIN advisory_opportunity o \
         ON (o.tenant_id,o.workspace_id,o.id)=(d.tenant_id,d.workspace_id,d.opportunity_id) \
         WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.request_key=$3",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace)
    .bind(authorized_params["request_id"].as_str().unwrap())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(after_skip, vec![("sealed".into(), "sent".into())]);
    let authorized_replay = route(
        &mut a,
        "command",
        "scope.advisory.request",
        authorized_params,
    )
    .await;
    assert_eq!(
        authorized_replay["opportunity_id"],
        authorized_result["opportunity_id"]
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    a.finish().await;
    b.finish().await;
    server.abort();
    let _ = server.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "owned disposable PostgreSQL 18.6 ledger110 and TECT_TEST_* required; no migration"]
async fn public_s01_request_decision_caller_and_distinct_verifier() {
    let (pool, runtime_url) = owned_ledger110().await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("scope-public-ledger110.sock");
    let runtime = tagged_url(&runtime_url, &format!("scope-s01-{}", Uuid::new_v4()));
    let store = PgStore::connect(&runtime, 4).await.unwrap();
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let workspace_key = format!("scope-s01-{}", Uuid::new_v4());
    let (expected_workspace, keys) = signed_fixture_budget(
        &store,
        &enrollment,
        &workspace_key,
        AdvisoryBudgetCeilings {
            provider_calls: 2,
            input_tokens: 1_000,
            output_tokens: 1_000,
            request_utf8_bytes: 2_000_000,
            elapsed_monotonic_ms: 120_000,
            retry_dispatches: 1,
        },
    )
    .await;
    let trusted = store.with_budget_owner_keys(keys);
    let authority = Arc::new(PgScopeAuthorityObserver::new(
        trusted.clone(),
        Arc::new(tect_host::StaticCandidateGuidance),
    ));
    let supplier = Arc::new(PgScopeAuthoredManifestSupplier::new(
        trusted.clone(),
        authority.clone(),
    ));
    let calls = Arc::new(AtomicUsize::new(0));
    let service = Arc::new(WorkspaceService::new_with_scope_advisory_adapters(
        Arc::new(trusted),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
        authority,
        supplier,
        Arc::new(SyntheticBudget),
        Arc::new(FakeJev {
            calls: calls.clone(),
            pause: None,
        }),
    ));
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    let config_path = root.join("host.json");
    host_file(&config_path, &enrollment.auth);
    let owner_session = Uuid::new_v4().to_string();
    let mut owner = Mcp::start(&socket, &config_path, &owner_session, &workspace_key).await;
    let (context, prior) = ready_source_candidate(&mut owner, &repo).await;
    let candidate_set = id(&context["candidate_set"]["id"]);
    let revision = context["candidate_set"]["revision"].as_i64().unwrap();
    let snapshot = context["snapshot"]["id"].clone();
    let input_cursor = context["candidate_set"]["input_cursor"].as_i64().unwrap();

    let disabled = route(
        &mut owner,
        "command",
        "scope.advisory.request",
        json!({"request_id":Uuid::new_v4(),"candidate_set_id":candidate_set}),
    )
    .await;
    assert_eq!(disabled["state"], "no_call");
    assert_eq!(disabled["reason"], "workspace_disabled");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let configured = route(
        &mut owner,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":0,"mode":"optional",
            "provider_profile_ref":{"id":"fixture-profile"},
            "model_configuration":{"model":"fixture-model"}
        }),
    )
    .await;
    let workspace = id(&configured["workspace_id"]);
    assert_eq!(workspace, expected_workspace);
    let skipped = route(
        &mut owner,
        "command",
        "scope.advisory.request",
        json!({"request_id":Uuid::new_v4(),"candidate_set_id":candidate_set,
            "request_preference":"skip"}),
    )
    .await;
    assert_eq!(skipped["state"], "no_call");
    assert_eq!(skipped["reason"], "request_skip");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for no_call in [&disabled, &skipped] {
        let detail = route(
            &mut owner,
            "query",
            "candidate.advisory.get",
            json!({"candidate_set_id":candidate_set,"opportunity_id":no_call["opportunity_id"]}),
        )
        .await;
        assert!(detail.get("scope_decomposition").is_none());
        assert!(detail["dispatches"].as_array().unwrap().is_empty());
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3"
        ).bind(enrollment.tenant_id).bind(workspace).bind(id(&no_call["opportunity_id"]))
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 0);
    }

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
        .map(|item| id(&item["input"]["source_ref_id"]))
        .collect();
    let source_ref = *refs.first().expect("planning input source");
    refs.extend(
        program["program"]["field_refs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| id(&field["id"])),
    );
    refs.sort_unstable();
    refs.dedup();
    let baseline_draft = authored_draft(source_ref, &prior, "Cohesive preview diagnosis");
    let partition_draft = authored_draft(source_ref, &prior, "Partitioned preview diagnosis");
    let request_id = Uuid::new_v4();
    let params = json!({"request_id":request_id,"candidate_set_id":candidate_set,
    "authored_scope_set":{"expected_candidate_set_revision":revision,
        "baseline_key":"baseline","alternatives":[
            {"key":"baseline","kind":"cohesive","draft":baseline_draft,
                "covered_source_ref_ids":refs},
            {"key":"partition","kind":"partitioned","draft":partition_draft,
                "covered_source_ref_ids":refs}
        ]}});
    let advised = route(
        &mut owner,
        "command",
        "scope.advisory.request",
        params.clone(),
    )
    .await;
    assert_eq!(advised["state"], "advised", "{advised}");
    assert_eq!(advised["provider_called"], true);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let opportunity = id(&advised["opportunity_id"]);
    let detail = route(
        &mut owner,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":candidate_set,"opportunity_id":opportunity}),
    )
    .await;
    let manifest = &detail["scope_decomposition"]["manifest"];
    let advice = &detail["scope_decomposition"]["advice"];
    assert_eq!(manifest["emitted"].as_array().unwrap().len(), 2);
    assert!(manifest["rejected"].as_array().unwrap().is_empty());
    assert_eq!(advice["items"].as_array().unwrap().len(), 2);
    assert_eq!(detail["dispatches"].as_array().unwrap().len(), 1);
    let baseline = &manifest["emitted"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "cohesive")
        .unwrap();
    let partition = &manifest["emitted"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "partitioned")
        .unwrap();
    assert_eq!(baseline["kind"], "cohesive");
    assert_eq!(partition["kind"], "partitioned");
    let selected_id = baseline["id"].clone();
    assert_eq!(selected_id, manifest["baseline_id"]);
    let mut obligations = manifest["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    obligations.sort_unstable();
    assert!(!obligations.is_empty());
    for alternative in [baseline, partition] {
        let mut coverage = alternative["coverage"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["obligation_id"].as_str().unwrap())
            .collect::<Vec<_>>();
        coverage.sort_unstable();
        assert_eq!(
            coverage, obligations,
            "each authored option covers every frozen obligation"
        );
    }
    assert!(
        advice["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["alternative_id"] == selected_id && item["choice"] == "preferred")
    );
    assert!(
        advice["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["alternative_id"] == partition["id"]
                && item["choice"] == "non_preferred")
    );
    let items = manifest["emitted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            json!({
                "alternative_id":item["id"],
                "state":if item["id"] == selected_id { "selected" } else { "not_selected" }
            })
        })
        .collect::<Vec<_>>();
    let replay = route(&mut owner, "command", "scope.advisory.request", params).await;
    assert_eq!(replay["opportunity_id"], advised["opportunity_id"]);
    assert_eq!(replay["advice_id"], advised["advice_id"]);
    assert_eq!(
        replay["provider_called"], true,
        "replay reports the persisted historical call"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let disposition = route(
        &mut owner,
        "command",
        "scope.advisory.disposition",
        json!({
            "opportunity_id":opportunity,"candidate_set_id":candidate_set,
            "request_id":Uuid::new_v4(),"advice_id":advice["id"],"expected_revision":0,
            "action":"accept","selected_id":selected_id,"items":items,
            "rationale":"Select the cohesive source-authored option"
        }),
    )
    .await;
    let save_request = Uuid::new_v4();
    let save = json!({"kind":"draft","candidate_set_id":candidate_set,
        "revision":revision,"snapshot_id":snapshot,"input_cursor":input_cursor,
        "request_id":save_request,"selected_advisory":{
            "opportunity_id":opportunity,"disposition_id":disposition["id"],
            "selected_id":selected_id,"alternative_key":"baseline"
        },"draft":baseline_draft});
    let mut wrong_session = Mcp::start(
        &socket,
        &config_path,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    route(&mut wrong_session, "command", "workspace.open", json!({})).await;
    let refused = route_error(
        &mut wrong_session,
        "command",
        "scope.candidates.save",
        save.clone(),
    )
    .await;
    assert_eq!(refused["error"]["code"], "stale_context");
    let saved = route(&mut owner, "command", "scope.candidates.save", save).await;
    assert_eq!(saved["context"]["candidate_set"]["revision"], revision + 1);
    assert_eq!(
        saved["context"]["candidate_set"]["status"],
        "review_required"
    );
    let (owner_actor, owner_session_id): (Uuid, Uuid) = sqlx::query_as(
        "SELECT h.principal_id,s.id FROM agent_sessions s JOIN hosts h \
         ON (h.tenant_id,h.id)=(s.tenant_id,s.host_id) \
         WHERE s.tenant_id=$1 AND s.native_session_id=$2",
    )
    .bind(enrollment.tenant_id)
    .bind(&owner_session)
    .fetch_one(&pool)
    .await
    .unwrap();
    let (link, target_revision, status, link_actor, link_session): (Uuid, i64, String, Uuid, Uuid) =
        sqlx::query_as(
            "SELECT l.link_id,l.caller_result_revision,p.status,l.actor_id,l.session_id \
         FROM advisory_scope_caller_link l JOIN advisory_scope_preservation_receipt p \
         ON (p.tenant_id,p.workspace_id,p.receipt_id)= \
            (l.tenant_id,l.workspace_id,l.preservation_receipt_id) \
         WHERE l.tenant_id=$1 AND l.workspace_id=$2 AND l.opportunity_id=$3 \
         AND l.candidate_set_id=$4 AND l.request_id=$5",
        )
        .bind(enrollment.tenant_id)
        .bind(workspace)
        .bind(opportunity)
        .bind(candidate_set)
        .bind(save_request)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "passed");
    assert_eq!((link_actor, link_session), (owner_actor, owner_session_id));
    assert_eq!(target_revision, revision + 1);

    let verifier = admin::prepare_verifier_enrollment(&pool, enrollment.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    assert_ne!(verifier.principal_id, owner_actor);
    let verifier_file = root.join("verifier-host.json");
    host_file(&verifier_file, &verifier.auth);
    let mut verifier_mcp = Mcp::start(
        &socket,
        &verifier_file,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    route(&mut verifier_mcp, "command", "workspace.open", json!({})).await;
    let verification = json!({"request_id":Uuid::new_v4(),"opportunity_id":opportunity,
        "candidate_set_id":candidate_set,"caller_link_id":link,
        "caller_receipt_request_id":save_request,"target_revision":target_revision});
    assert_eq!(
        route_error(
            &mut owner,
            "command",
            "candidate.advisory.verify",
            verification.clone()
        )
        .await["error"]["code"],
        "forbidden"
    );
    let observed = route(
        &mut verifier_mcp,
        "command",
        "candidate.advisory.verify",
        verification,
    )
    .await;
    assert_eq!(observed["observation"]["status"], "passed");
    assert_eq!(
        observed["observation"]["qualification"],
        "independently_observed"
    );
    assert_eq!(
        observed["observation"]["actor_id"],
        verifier.principal_id.to_string()
    );
    assert_eq!(observed["establishes_independent_approval"], false);
    assert_eq!(observed["establishes_current_acceptance"], false);
    let verifier_detail = route(
        &mut verifier_mcp,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":candidate_set,"opportunity_id":opportunity}),
    )
    .await;
    assert!(verifier_detail.get("scope_decomposition").is_none());
    assert_eq!(
        verifier_detail["opportunity"]["selected_save_observation"]["status"],
        "passed"
    );
    let audit = route(
        &mut owner,
        "query",
        "candidate.advisory.audit",
        json!({"candidate_set_id":candidate_set,"limit":25}),
    )
    .await;
    assert_eq!(audit["aggregate"]["authorized_attempts"], 1);
    assert_eq!(audit["aggregate"]["no_call_opportunities"], 2);
    assert!(audit.to_string().contains("independently_observed"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    wrong_session.finish().await;
    verifier_mcp.finish().await;
    owner.finish().await;
    server.abort();
    let _ = server.await;
}

#[path = "scope_decomposition_public_ledger110/s01_live.rs"]
mod s01_live;
