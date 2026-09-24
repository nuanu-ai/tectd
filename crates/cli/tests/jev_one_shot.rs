//! Ignored, disposable-PostgreSQL integration harness. See `jev_one_shot.md`.
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::Path, sync::Arc};
use support::{id, ready_source_candidate, repository, route};
use tect_application::{
    AuthoredScopeAlternative, AuthoredScopeSet, RunScopeAdvisory, ScopeAdviceProvider,
    ScopeAdviceProviderContext, ScopeAuthoredManifestRequest, ScopeAuthorityObserver,
    ScopeAuthorityOutcome, ScopeAuthorityRequest, ScopeBudgetPolicy, ScopeBudgetPolicyEvaluation,
    ScopeBudgetRequest, ScopeManifestSupplier, Sha256ScopeDigest, WorkspaceService,
};
use tect_domain::{
    AdvisoryOpportunityState, AdvisoryRequestPreference, RequestContext, ScopeAdviceRequest,
    ScopeDecompositionKind,
};
use tect_postgres::{PgScopeAuthoredManifestSupplier, PgScopeAuthorityObserver, PgStore, admin};
use uuid::Uuid;

const CALL_ID: &str = "tectd-jev-scope-evidence-2026-09-24-1";
const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const MODEL: &str = "jev-1.13.0";
// Outside the disposable fixture and isolated worktree so rebuilding or
// rerunning this test cannot quietly reset its send allowance.
const MARKER_PATH: &str = "/Users/tony/Work/Projects/nuanu-ai-lab/artifacts/jev-live-eval-20260919/tectd-jev-scope-evidence-2026-09-24-1.used";

/// This owner approval applies to one process invocation after its marker is
/// durably created. It does not install a reusable budget in tectd.
struct OneUseApproval(std::sync::atomic::AtomicBool);

#[async_trait::async_trait]
impl ScopeBudgetPolicy for OneUseApproval {
    async fn evaluate(
        &self,
        _: &ScopeBudgetRequest,
    ) -> tect_domain::Result<Option<ScopeBudgetPolicyEvaluation>> {
        Ok(
            (!self.0.swap(true, std::sync::atomic::Ordering::SeqCst)).then(|| {
                ScopeBudgetPolicyEvaluation {
                    policy_id: CALL_ID.into(),
                }
            }),
        )
    }
}

fn marker(path: &Path, digest: &str) {
    assert!(path.is_absolute(), "JEV_ONE_SHOT_MARKER must be absolute");
    let parent = path.parent().expect("marker parent");
    assert!(parent.is_dir(), "marker parent must already exist");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .expect("one-shot marker already exists or cannot be created; no call sent");
    writeln!(file, "call_id={CALL_ID}\nrequest_sha256={digest}").unwrap();
    file.sync_all().unwrap();
    fs::File::open(parent).unwrap().sync_all().unwrap();
}

fn draft(previous: &Value, source_ref: Uuid, title: &str) -> tect_domain::ScopeCandidateDraft {
    let goal = &previous["goals"][0];
    let candidate = &previous["candidates"][0];
    serde_json::from_value(json!({
        "boundary":"ongoing",
        "goals":[{"identity":{"id":goal["id"],"revision":goal["revision"]},
            "text":"Demonstrate why notification preview differs from saved settings",
            "source_ref_id":source_ref,
            "resolution":{"kind":"candidate","reference":{"id":candidate["id"]}}}],
        "evidence":[],
        "candidates":[{"identity":{"id":candidate["id"],"revision":candidate["revision"]},
            "change_rationale":"Compare bounded Scope decompositions for the same source",
            "title":title,
            "outcome":"Preview deviation is explained and a bounded correction is selected",
            "trigger":"The preview differs from saved notification settings",
            "delivered_behavior":"A reproducible cause and correction decision are available",
            "proof":"Recorded source and a repeatable preview check support the decision",
            "includes":["diagnose preview rendering","select correction"],
            "excludes":["deployment","unrelated notification flows"],
            "dependencies":[],"coverage_goals":[{"id":goal["id"]}],"evidence":[]}],
        "blockers":[],"protected_changes":[]
    }))
    .expect("valid source-authored draft")
}

fn provider(credential: String) -> tect_host::JevScopeAdviceProvider {
    tect_host::JevScopeAdviceProvider::new(
        tect_host::JevScopeAdviceConfig {
            profile: CALL_ID.into(),
            endpoint: ENDPOINT.parse().unwrap(),
            model: MODEL.into(),
            timeout: std::time::Duration::from_secs(15),
            maximum_request_bytes: 262_144,
            maximum_response_bytes: 65_536,
        },
        credential,
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit JEV_ONE_SHOT_MODE=preflight or send; requires disposable PostgreSQL 18 and TECT_TEST_*"]
async fn one_shot_real_jev_evidence() {
    let mode = std::env::var("JEV_ONE_SHOT_MODE").expect("set preflight or send");
    assert!(matches!(mode.as_str(), "preflight" | "send"));
    // Resolve the exact marker before constructing any provider or DB fixture.
    let marker_path = if mode == "send" {
        let path = std::path::PathBuf::from(MARKER_PATH);
        assert!(path.is_absolute() && !path.exists());
        Some(path)
    } else {
        None
    };
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("admin URL required");
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").expect("runtime URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("runtime role required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let version: String = sqlx::query_scalar("SHOW server_version_num")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version.parse::<i32>().unwrap() / 10_000, 18);

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("one-shot.sock");
    let runtime = tagged_url(&runtime_url, &format!("jev-one-shot-{}", Uuid::new_v4()));
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config_path = root.join("host.json");
    host_file(&config_path, &enrollment.auth);
    let workspace_key = format!("jev-one-shot-{}", Uuid::new_v4());
    let native_session = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config_path, &native_session, &workspace_key).await;
    let (context, candidate) = ready_source_candidate(&mut client, &repo).await;
    let candidate_set_id = id(&context["candidate_set"]["id"]);
    let config = route(&mut client, "query", "workspace.advisory.config", json!({})).await;
    assert_eq!(config["mode"], "disabled");
    let workspace_id = id(&config["workspace_id"]);
    let configured = route(
        &mut client,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":0,"mode":"optional",
            "provider_profile_ref":{"id":CALL_ID},
            "model_configuration":{"model":MODEL}
        }),
    )
    .await;
    assert_eq!(configured["revision"], 1);
    assert_eq!(configured["mode"], "optional");
    let previous: Value = sqlx::query_scalar(
        "SELECT payload FROM scope_candidate_drafts WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 ORDER BY set_revision DESC LIMIT 1"
    ).bind(enrollment.tenant_id).bind(workspace_id).bind(candidate_set_id)
        .fetch_one(&pool).await.unwrap();
    assert_eq!(previous["candidates"][0]["id"], candidate["id"]);
    let mut refs: Vec<Uuid> = context["snapshot"]["source_refs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| id(&item["id"]))
        .collect();
    refs.sort();
    assert!(!refs.is_empty());
    let planning_ref = context["snapshot"]["source_refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "planning_input")
        .unwrap();
    let source_ref = id(&planning_ref["id"]);
    let revision = context["candidate_set"]["revision"].as_i64().unwrap();
    let authored = AuthoredScopeSet {
        expected_candidate_set_revision: revision,
        baseline_key: "cohesive".into(),
        alternatives: vec![
            AuthoredScopeAlternative {
                key: "cohesive".into(),
                kind: ScopeDecompositionKind::Cohesive,
                draft: draft(
                    &previous,
                    source_ref,
                    "Diagnose preview and select correction",
                ),
                covered_source_ref_ids: refs.clone(),
            },
            AuthoredScopeAlternative {
                key: "partitioned".into(),
                kind: ScopeDecompositionKind::Partitioned,
                draft: draft(
                    &previous,
                    source_ref,
                    "Separate preview diagnosis from correction decision",
                ),
                covered_source_ref_ids: refs,
            },
        ],
    };
    authored.validate().unwrap();
    let (session_id, actor_id): (Uuid, Uuid) = sqlx::query_as(
        "SELECT s.id,h.principal_id FROM agent_sessions s JOIN hosts h ON (s.tenant_id,s.host_id)=(h.tenant_id,h.id) WHERE s.tenant_id=$1 AND s.native_session_id=$2"
    ).bind(enrollment.tenant_id).bind(&native_session).fetch_one(&pool).await.unwrap();
    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
    let authority = Arc::new(PgScopeAuthorityObserver::new(
        store.clone(),
        Arc::new(tect_host::StaticCandidateGuidance),
    ));
    let observation = authority
        .observe(&ScopeAuthorityRequest {
            tenant_id: enrollment.tenant_id,
            workspace_id,
            actor_id,
            session_id,
            candidate_set_id,
        })
        .await
        .unwrap();
    let ScopeAuthorityOutcome::Authorized(observation) = observation else {
        panic!("source must be authorized")
    };
    let supplier = Arc::new(PgScopeAuthoredManifestSupplier::new(
        store.clone(),
        authority.clone(),
    ));
    let manifest = supplier
        .supply_authored(&ScopeAuthoredManifestRequest {
            tenant_id: enrollment.tenant_id,
            observation,
            authored_scope_set: authored.clone(),
        })
        .await
        .unwrap();
    manifest.validate(&Sha256ScopeDigest).unwrap();
    let request = ScopeAdviceRequest::from_manifest(&Sha256ScopeDigest, &manifest).unwrap();
    let provider_context = ScopeAdviceProviderContext::from_manifest(&request, &manifest).unwrap();
    let preflight_provider = provider("preflight-placeholder-never-sent".into());
    let prepared = preflight_provider
        .prepare_context(&provider_context)
        .unwrap();
    assert_eq!(prepared.destination(), ENDPOINT);
    assert_eq!(prepared.model(), MODEL);
    assert!(prepared.body_length() > 0 && prepared.body_length() <= 262_144);
    let body: Value = serde_json::from_slice(prepared.body()).unwrap();
    assert_eq!(body["model"], MODEL);
    assert_eq!(body["state"]["emitted"].as_array().unwrap().len(), 2);
    assert_eq!(
        body["state"]["request"]["alternatives"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let digest = prepared.body_sha256().to_owned();

    let request_context = RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: native_session,
        workspace_key,
    };
    let no_call_service = WorkspaceService::new_with_scope_advisory_adapters(
        Arc::new(store.clone()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
        authority.clone(),
        supplier.clone(),
        Arc::new(tect_application::DenyScopeBudget),
        Arc::new(preflight_provider),
    );
    let no_call = no_call_service
        .run_scope_advisory(
            &request_context,
            &RunScopeAdvisory {
                request_id: Uuid::new_v4(),
                candidate_set_id,
                session_preference: AdvisoryRequestPreference::UseWorkspace,
                request_preference: AdvisoryRequestPreference::UseWorkspace,
                authored_scope_set: Some(authored.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(no_call.opportunity.state, AdvisoryOpportunityState::NoCall);
    assert!(!no_call.opportunity.provider_called);
    let no_call_dispatches: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE opportunity_id=$1")
            .bind(no_call.opportunity.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(no_call_dispatches, 0);
    println!(
        "preflight call_id={CALL_ID} workspace={workspace_id} candidate={candidate_set_id} body_bytes={} body_sha256={digest} no_call_opportunity={}",
        prepared.body_length(),
        no_call.opportunity.id
    );
    if mode == "preflight" {
        return;
    }

    // Read only process environment; never parse a .env file or print the key.
    let key = std::env::var("TYPESAFE_API_KEY").expect("TYPESAFE_API_KEY process env required");
    assert!(!key.trim().is_empty(), "TYPESAFE_API_KEY must be nonempty");
    let live_provider = provider(key);
    let marker_path = marker_path.unwrap();
    marker(&marker_path, &digest);
    let service = WorkspaceService::new_with_scope_advisory_adapters(
        Arc::new(store),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
        authority,
        supplier,
        Arc::new(OneUseApproval(std::sync::atomic::AtomicBool::new(false))),
        Arc::new(live_provider),
    );
    let outcome = service
        .run_scope_advisory(
            &request_context,
            &RunScopeAdvisory {
                request_id: Uuid::new_v4(),
                candidate_set_id,
                session_preference: AdvisoryRequestPreference::UseWorkspace,
                request_preference: AdvisoryRequestPreference::UseWorkspace,
                authored_scope_set: Some(authored),
            },
        )
        .await
        .unwrap();
    let attempts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE opportunity_id=$1")
            .bind(outcome.opportunity.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        attempts <= 1,
        "one-shot harness created multiple dispatches"
    );
    println!(
        "live call_id={CALL_ID} opportunity={} state={:?} reason={:?} dispatches={attempts} advice_present={} marker={}",
        outcome.opportunity.id,
        outcome.opportunity.state,
        outcome.opportunity.primary_reason,
        outcome.advice.is_some(),
        marker_path.display()
    );
}

#[test]
fn marker_is_exclusive_and_retained() {
    let dir = private_temp();
    let path = dir.path().join(format!("{CALL_ID}.used"));
    marker(&path, "abc");
    assert!(path.exists());
    let second = std::panic::catch_unwind(|| marker(&path, "def"));
    assert!(second.is_err());
    assert!(
        fs::read_to_string(path)
            .unwrap()
            .contains("request_sha256=abc")
    );
}
