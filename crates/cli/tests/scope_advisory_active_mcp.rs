//! Public MCP active-mode proof with a test-only provider and disposable PG18.
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
mod support;

use async_trait::async_trait;
use recovery_support::{Mcp, host_file, private_temp, tagged_url};
use serde_json::json;
use sqlx::PgPool;
use std::os::unix::fs::PermissionsExt;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use support::{id, ready_source_candidate, repository, route};
use tect_application::{
    PreparedScopeAdviceAttempt, ScopeAdviceProvider, ScopeAdviceProviderContext,
    ScopeAdviceProviderError, ScopeAdviceProviderObservation, ScopeAdviceProviderRequest,
    ScopeBudgetPolicy, ScopeBudgetPolicyEvaluation, ScopeBudgetRequest, StartedScopeDispatchPermit,
    WorkspaceService,
};
use tect_domain::{
    AdvisoryDispatchOutcome, AdvisorySendCertainty, ConfidenceBasisPoints,
    NormalizedScopeAdviceAnswer, NormalizedScopeAdviceAnswers, ScopeAdviceChoice,
    ScopeAdviceScoreBand,
};
use tect_postgres::{PgScopeAuthoredManifestSupplier, PgScopeAuthorityObserver, PgStore, admin};
use tokio::net::UnixListener;
use uuid::Uuid;

struct SyntheticPositiveBudget;

#[async_trait]
impl ScopeBudgetPolicy for SyntheticPositiveBudget {
    async fn evaluate(
        &self,
        _: &ScopeBudgetRequest,
    ) -> tect_domain::Result<Option<ScopeBudgetPolicyEvaluation>> {
        Ok(Some(ScopeBudgetPolicyEvaluation {
            policy_id: "test-only-synthetic-positive".into(),
        }))
    }
}

struct FakeProvider(Arc<AtomicUsize>);

#[async_trait]
impl ScopeAdviceProvider for FakeProvider {
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
        self.0.fetch_add(1, Ordering::SeqCst);
        let answers = request
            .request
            .alternatives
            .iter()
            .enumerate()
            .map(|(index, alternative)| NormalizedScopeAdviceAnswer {
                alternative_id: alternative.id.clone(),
                choice: ScopeAdviceChoice::Preferred,
                score: if index == 0 {
                    ScopeAdviceScoreBand::Fit
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
            raw_response_ref: Some("fixture:response:1".into()),
            failure_reason: None,
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires disposable PostgreSQL 18.6 and TECT_TEST_*"]
async fn public_mcp_active_request_persists_advice_and_replay_does_not_redispatch() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let version: String = sqlx::query_scalar("SHOW server_version_num")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version.parse::<i32>().unwrap(), 180_006);

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("scope-advisory-active.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-active-{}", Uuid::new_v4()));
    let store = PgStore::connect(&runtime, 4).await.unwrap();
    let authority = Arc::new(PgScopeAuthorityObserver::new(
        store.clone(),
        Arc::new(tect_host::StaticCandidateGuidance),
    ));
    let supplier = Arc::new(PgScopeAuthoredManifestSupplier::new(
        store.clone(),
        authority.clone(),
    ));
    let calls = Arc::new(AtomicUsize::new(0));
    let service = Arc::new(WorkspaceService::new_with_scope_advisory_adapters(
        Arc::new(store),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
        authority,
        supplier,
        Arc::new(SyntheticPositiveBudget),
        Arc::new(FakeProvider(calls.clone())),
    ));
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));

    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config_path = root.join("host.json");
    host_file(&config_path, &enrollment.auth);
    let native_session = Uuid::new_v4().to_string();
    let workspace_key = format!("scope-active-{}", Uuid::new_v4());
    let mut client = Mcp::start(&socket, &config_path, &native_session, &workspace_key).await;
    let (context, prior_candidate) = ready_source_candidate(&mut client, &repo).await;
    let candidate_set_id = id(&context["candidate_set"]["id"]);
    let revision = context["candidate_set"]["revision"].as_i64().unwrap();
    let inputs = client
        .call(
            "candidate_context",
            json!({"candidate_set_id":candidate_set_id,"view":"inputs","limit":25}),
        )
        .await;
    let program = client
        .call(
            "candidate_context",
            json!({"candidate_set_id":candidate_set_id,"view":"program","limit":25}),
        )
        .await;
    let mut source_refs: Vec<Uuid> = inputs["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| id(&item["input"]["source_ref_id"]))
        .collect();
    let source_ref = *source_refs.first().expect("planning input source");
    source_refs.extend(
        program["program"]["field_refs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| id(&field["id"])),
    );
    source_refs.sort_unstable();
    source_refs.dedup();

    let configured = route(
        &mut client,
        "command",
        "workspace.advisory.configure",
        json!({"expected_revision":0,"mode":"optional",
            "provider_profile_ref":{"id":"fixture-profile"},
            "model_configuration":{"model":"fixture-model"}}),
    )
    .await;
    assert_eq!(configured["revision"], 1);
    let workspace_id = id(&configured["workspace_id"]);
    let draft = |title: &str| {
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
            "candidate_id":prior_candidate["id"],"revision":prior_candidate["revision"],
            "reason":"Compare this authored option with the prior candidate",
            "replacements":[{"local":"candidate"}]
        }]})
    };
    let request_id = Uuid::new_v4();
    let params = json!({"request_id":request_id,"candidate_set_id":candidate_set_id,
    "authored_scope_set":{
        "expected_candidate_set_revision":revision,"baseline_key":"baseline",
        "alternatives":[
            {"key":"baseline","kind":"cohesive","draft":draft("Cohesive diagnosis"),
             "covered_source_ref_ids":source_refs},
            {"key":"alternative","kind":"cohesive","draft":draft("Alternative diagnosis"),
             "covered_source_ref_ids":source_refs}
        ]
    }});
    let first = route(
        &mut client,
        "command",
        "scope.advisory.request",
        params.clone(),
    )
    .await;
    assert_eq!(first["state"], "advised", "{first}");
    assert_eq!(first["reason"], "provider_response");
    assert_eq!(first["provider_called"], true);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let opportunity_id = id(&first["opportunity_id"]);
    let advice_id = first["advice_id"].as_str().expect("persisted typed advice");

    let owned = route(
        &mut client,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":candidate_set_id,"opportunity_id":opportunity_id}),
    )
    .await;
    assert_eq!(owned["opportunity"]["id"], first["opportunity_id"]);
    assert_eq!(owned["scope_decomposition"]["advice"]["id"], advice_id);
    assert_eq!(
        owned["scope_decomposition"]["advice"]["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        owned["scope_decomposition"]["manifest"]["emitted"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(owned["dispatches"].as_array().unwrap().len(), 1);
    assert_eq!(owned["dispatches"][0]["state"], "sealed");

    let replay = route(&mut client, "command", "scope.advisory.request", params).await;
    assert_eq!(replay["opportunity_id"], first["opportunity_id"]);
    assert_eq!(replay["advice_id"], first["advice_id"]);
    assert_eq!(replay["provider_called"], false);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let (dispatches, sealed): (i64, i64) = sqlx::query_as(
        "SELECT count(*),count(*) FILTER (WHERE state='sealed' AND outcome='provider_response' \
         AND send_certainty='sent' AND send_started_at IS NOT NULL AND sealed_at IS NOT NULL) \
         FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(opportunity_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((dispatches, sealed), (1, 1));

    client.finish().await;
    server.abort();
    let _ = server.await;
}
