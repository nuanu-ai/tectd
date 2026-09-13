//! Trusted application guidance changes stale the head and retain old snapshot bodies.
mod recovery_support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use tect_application::{CandidateGuidance, WorkspaceService};
use tect_domain::{
    CandidateContextQuery, CandidateContextView, CandidateMethodSnapshot, CandidateRuleSnapshot,
    CandidateSetStatus, CandidateSnapshotMaterial, Program, RefreshCandidateSet, RequestContext,
    Result, WorktreeSummary,
};
use tect_host::{CandidateEncoding, GitSourceInspector, LocalSetupFiles};
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

struct Guidance {
    revision: &'static str,
    selected_digest: String,
    method_body: &'static str,
    rule_body: &'static str,
}

impl CandidateGuidance for Guidance {
    fn snapshot(
        &self,
        program: Program,
        selected_worktrees: Vec<WorktreeSummary>,
    ) -> Result<CandidateSnapshotMaterial> {
        let changed = self.revision == "new";
        Ok(CandidateSnapshotMaterial {
            program,
            selected_worktrees,
            selected_sources_digest: self.selected_digest.clone(),
            method: CandidateMethodSnapshot {
                id: "trusted-guidance-fixture".into(),
                revision: self.revision.into(),
                digest: if changed { "2" } else { "1" }.repeat(64),
                body: self.method_body.into(),
                origin_refs: vec![format!("test:method@{}", self.revision)],
            },
            registry_revision: self.revision.into(),
            registry_digest: if changed { "4" } else { "3" }.repeat(64),
            rules: vec![CandidateRuleSnapshot {
                id: "trusted-rule".into(),
                revision: self.revision.into(),
                text: self.rule_body.into(),
                origin_refs: vec![format!("test:rule@{}", self.revision)],
                applicability: vec!["purpose=trusted-test".into()],
            }],
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn method_and_registry_change_require_refresh_and_retain_old_bodies() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("guidance.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-guidance-{}", Uuid::new_v4()));
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace = format!("guidance-{}", Uuid::new_v4().simple());
    let native = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config, &native, &workspace).await;
    client.call("open_workspace", json!({})).await;
    let started = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"Create guidance revision fixture."}),
        )
        .await;
    let program = Uuid::parse_str(started["program"]["id"].as_str().unwrap()).unwrap();
    client
        .call(
            "save_program",
            json!({
                "program_id":program,"revision":1,"input_cursor":1,"name":"Guidance revision",
                "intent":"Retain exact planning instructions","basis":"Trusted test fixture",
                "boundaries":"Candidate snapshot only","constraints":"No public overrides",
                "success":"Old and new bodies remain independently readable","complete":true
            }),
        )
        .await;

    let service = WorkspaceService::new(
        Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap()),
        Arc::new(GitSourceInspector),
        Arc::new(LocalSetupFiles),
    );
    let context = RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: native,
        workspace_key: workspace,
    };
    let guard = CandidateEncoding {
        capacity: 8 * 1024 * 1024,
    };
    let created = client
        .call(
            "begin_candidate_set",
            json!({"request_id":Uuid::new_v4(),"program_id":program,"program_revision":2,
                "boundary":"ongoing","input":"Plan only the captured guidance fixture."}),
        )
        .await;
    let candidate_set =
        Uuid::parse_str(created["context"]["candidate_set"]["id"].as_str().unwrap()).unwrap();
    let snapshot = &created["context"]["snapshot"];
    let new = Guidance {
        revision: "new",
        selected_digest: snapshot["selected_sources_digest"]
            .as_str()
            .unwrap()
            .to_owned(),
        method_body: "New trusted candidate method body.",
        rule_body: "New trusted candidate rule body.",
    };
    let input_ref = snapshot["source_refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["kind"] == "planning_input")
        .unwrap()["id"]
        .clone();
    let saved = client
        .call(
            "save_candidate_set",
            json!({
                "kind":"draft","candidate_set_id":candidate_set,"revision":1,
                "snapshot_id":snapshot["id"],"input_cursor":1,"request_id":Uuid::new_v4(),
                "draft":{"boundary":"ongoing","goals":[{"identity":{"local":"goal"},
                    "text":"Retain guidance history","source_ref_id":input_ref,
                    "resolution":{"kind":"candidate","reference":{"local":"candidate"}}}],
                    "evidence":[],"candidates":[{"identity":{"local":"candidate"},
                    "title":"Guidance retention","outcome":"Old guidance remains readable",
                    "trigger":"Refresh guidance","delivered_behavior":"Retain both bodies",
                    "proof":"Query immutable snapshots","coverage_goals":[{"local":"goal"}]}],
                    "blockers":[]}}
            ),
        )
        .await;
    let candidate = saved["draft"]["candidates"][0]["id"].clone();
    let ready = client.call("save_candidate_set", json!({
        "kind":"review","candidate_set_id":candidate_set,"revision":2,
        "snapshot_id":snapshot["id"],"input_cursor":1,"request_id":Uuid::new_v4(),
        "review":{"verdict":"ready","summary":"The initial candidate follows captured guidance.",
        "findings":[],"candidate_decisions":[{"candidate_id":candidate,"decision":"accept",
            "rationale":"Bounded and observable"}]}
    })).await;
    assert_eq!(ready["context"]["candidate_set"]["status"], "ready");
    let stale = service
        .candidate_context(
            &context,
            &CandidateContextQuery {
                candidate_set_id: candidate_set,
                view: CandidateContextView::Overview,
                draft_revision: None,
                after: None,
                limit: 25,
            },
            &new,
        )
        .await
        .unwrap();
    assert!(
        stale
            .context
            .stale_reasons
            .iter()
            .any(|value| value == "method")
    );
    assert!(
        stale
            .context
            .stale_reasons
            .iter()
            .any(|value| value == "rules")
    );
    let refreshed = service
        .refresh_candidate_set(
            &context,
            &RefreshCandidateSet {
                candidate_set_id: candidate_set,
                revision: 3,
                request_id: Uuid::new_v4(),
                program_revision: 2,
            },
            &new,
            &guard,
        )
        .await
        .unwrap();
    assert_eq!(refreshed.context.candidate_set.revision, 4);
    assert_eq!(
        refreshed.context.candidate_set.status,
        CandidateSetStatus::ReviewRequired
    );
    assert_eq!(refreshed.context.snapshot.method.revision, "new");
    assert_eq!(refreshed.reviews.len(), 1);

    let snapshots: Vec<(i64, String, String, String, serde_json::Value)> = sqlx::query_as(
        "SELECT sequence,method_revision,method_body,registry_revision,rules \
         FROM scope_candidate_snapshots WHERE candidate_set_id=$1 ORDER BY sequence",
    )
    .bind(candidate_set)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].1, "3");
    assert!(snapshots[0].2.contains("# TectD Scope candidates"));
    assert_eq!(snapshots[0].3, "3");
    assert_eq!(snapshots[0].4.as_array().unwrap().len(), 4);
    assert_eq!(snapshots[1].1, "new");
    assert_eq!(snapshots[1].2, "New trusted candidate method body.");
    assert_eq!(snapshots[1].3, "new");
    assert_eq!(
        snapshots[1].4[0]["text"],
        "New trusted candidate rule body."
    );
    let review_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_candidate_reviews WHERE candidate_set_id=$1",
    )
    .bind(candidate_set)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(review_count, 1);

    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
