mod recovery_support;
#[path = "native_planning/support.rs"]
mod support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::sync::Arc;
use support::{id, ready_source_candidate, repository};
use tect_application::{CandidateGuidance, NativePlanningGuidance, WorkspaceService};
use tect_domain::{
    CandidateDecision, CandidateDecisionKind, CandidateMethodSnapshot, CandidateReviewDraft,
    CandidateRuleSnapshot, CandidateSetStatus, CandidateSnapshotMaterial, Error, OpenScope,
    OpenScopeOutcome, PipelineCatalogueEntry, PipelineCatalogueSnapshot, PipelineExecutionOwner,
    PipelineKind, Program, RefreshCandidateSet, RequestContext, Result, ReviewCandidateSet,
    ReviewVerdict, ScopeOpenBasis, SlicePlanningInput, SlicePlanningSnapshotMaterial, SliceResult,
    WorktreeSummary,
};
use tect_host::{CandidateEncoding, GitSourceInspector, LocalSetupFiles, NativePlanningEncoding};
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

struct SourceGuidance {
    revision: &'static str,
    selected_digest: String,
}

fn digest(body: &str) -> String {
    Sha256::digest(body.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl CandidateGuidance for SourceGuidance {
    fn snapshot(
        &self,
        program: Program,
        selected_worktrees: Vec<WorktreeSummary>,
    ) -> Result<CandidateSnapshotMaterial> {
        let method_body = format!("source method {}", self.revision);
        Ok(CandidateSnapshotMaterial {
            program,
            selected_worktrees,
            selected_sources_digest: self.selected_digest.clone(),
            method: CandidateMethodSnapshot {
                id: "source-drift-fixture".into(),
                revision: self.revision.into(),
                digest: digest(&method_body),
                body: method_body,
                origin_refs: vec!["test".into()],
            },
            registry_revision: self.revision.into(),
            registry_digest: self.revision.repeat(64),
            rules: vec![CandidateRuleSnapshot {
                id: "source-rule".into(),
                revision: self.revision.into(),
                text: format!("source rule {}", self.revision),
                origin_refs: vec!["test".into()],
                applicability: vec!["purpose=test".into()],
            }],
        })
    }
}

struct SliceGuidance;
impl NativePlanningGuidance for SliceGuidance {
    fn snapshot(
        &self,
        _basis: &ScopeOpenBasis,
        _inputs: &[SlicePlanningInput],
        _results: &[SliceResult],
    ) -> Result<SlicePlanningSnapshotMaterial> {
        let method_body = "slice method";
        let entries = PipelineKind::ALL
            .into_iter()
            .map(|kind| PipelineCatalogueEntry {
                kind,
                description: "provisional description".into(),
                implementation_status: "stub".into(),
                description_status: "provisional".into(),
                refinement_required: true,
                executable: false,
                default_delivery_mode: None,
                allowed_delivery_modes: Vec::new(),
                execution_owner: if kind == PipelineKind::PromoteToDurableKnowledge {
                    PipelineExecutionOwner::KnowledgeChange
                } else {
                    PipelineExecutionOwner::SlicePipelineRun
                },
                choose_when: "evidence matches".into(),
                do_not_choose_when: "evidence does not match".into(),
                expected_result: "bounded result".into(),
            })
            .collect();
        Ok(SlicePlanningSnapshotMaterial {
            method: CandidateMethodSnapshot {
                id: "slice-fixture".into(),
                revision: "1".into(),
                digest: digest(method_body),
                body: method_body.into(),
                origin_refs: vec!["test".into()],
            },
            registry_revision: "1".into(),
            registry_digest: "b".repeat(64),
            rules: (0..4)
                .map(|n| CandidateRuleSnapshot {
                    id: format!("rule-{n}"),
                    revision: "1".into(),
                    text: format!("rule body {n}"),
                    origin_refs: vec!["test".into()],
                    applicability: vec!["purpose=test".into()],
                })
                .collect(),
            catalogue: PipelineCatalogueSnapshot {
                revision: "1".into(),
                digest: "c".repeat(64),
                entries,
            },
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn new_scope_open_rejects_source_guidance_drift_but_receipt_replays() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("source-drift.sock");
    let runtime = tagged_url(&runtime_url, &format!("source-drift-{}", Uuid::new_v4()));
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace = format!("source-drift-{}", Uuid::new_v4());
    let native = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config, &native, &workspace).await;
    let (ready, candidate) = ready_source_candidate(&mut client, &repo).await;
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
    let source = SourceGuidance {
        revision: "d",
        selected_digest: ready["snapshot"]["selected_sources_digest"]
            .as_str()
            .unwrap()
            .into(),
    };
    let request = OpenScope {
        request_id: Uuid::new_v4(),
        candidate_set_id: id(&ready["candidate_set"]["id"]),
        candidate_set_revision: ready["candidate_set"]["revision"].as_i64().unwrap(),
        candidate_snapshot_id: id(&ready["snapshot"]["id"]),
        candidate_id: id(&candidate["id"]),
        candidate_revision: candidate["revision"].as_i64().unwrap(),
        task_context: Default::default(),
        consumed_knowledge: None,
    };
    let native_guard = NativePlanningEncoding {
        capacity: 8 * 1024 * 1024,
    };
    assert_eq!(
        service
            .scope_open(&context, &request, &source, &SliceGuidance, &native_guard,)
            .await
            .unwrap_err(),
        Error::StaleContext
    );
    let guard = CandidateEncoding {
        capacity: 8 * 1024 * 1024,
    };
    let refreshed = service
        .refresh_candidate_set(
            &context,
            &RefreshCandidateSet {
                candidate_set_id: request.candidate_set_id,
                revision: request.candidate_set_revision,
                request_id: Uuid::new_v4(),
                program_revision: ready["snapshot"]["program_revision"].as_i64().unwrap(),
                task_context: Default::default(),
            },
            &source,
            &guard,
        )
        .await
        .unwrap();
    assert_eq!(
        refreshed.context.candidate_set.status,
        CandidateSetStatus::ReviewRequired
    );
    let candidate_id = refreshed.draft.as_ref().unwrap().candidates[0].id;
    let reviewed = service
        .review_candidate_set(
            &context,
            &ReviewCandidateSet {
                candidate_set_id: request.candidate_set_id,
                revision: refreshed.context.candidate_set.revision,
                snapshot_id: refreshed.context.snapshot.id,
                input_cursor: refreshed.context.candidate_set.input_cursor,
                request_id: Uuid::new_v4(),
                review: CandidateReviewDraft {
                    verdict: ReviewVerdict::Ready,
                    summary: "Refreshed source guidance reviewed".into(),
                    findings: vec![],
                    candidate_decisions: vec![CandidateDecision {
                        candidate_id,
                        decision: CandidateDecisionKind::Accept,
                        rationale: "Still bounded".into(),
                    }],
                    protected_change_reviews: vec![],
                },
                consumed_knowledge: None,
            },
            &source,
            &guard,
        )
        .await
        .unwrap();
    assert_eq!(
        reviewed.context.candidate_set.status,
        CandidateSetStatus::Ready
    );
    let current = OpenScope {
        request_id: Uuid::new_v4(),
        candidate_set_revision: reviewed.context.candidate_set.revision,
        candidate_snapshot_id: reviewed.context.snapshot.id,
        ..request
    };
    let created = service
        .scope_open(&context, &current, &source, &SliceGuidance, &native_guard)
        .await
        .unwrap();
    assert!(matches!(created, OpenScopeOutcome::Created(_)));
    let newer = SourceGuidance {
        revision: "e",
        selected_digest: source.selected_digest.clone(),
    };
    assert!(matches!(
        service
            .scope_open(&context, &current, &newer, &SliceGuidance, &native_guard,)
            .await
            .unwrap(),
        OpenScopeOutcome::Replay(_)
    ));
    let fresh_request = OpenScope {
        request_id: Uuid::new_v4(),
        ..current
    };
    assert_eq!(
        service
            .scope_open(
                &context,
                &fresh_request,
                &newer,
                &SliceGuidance,
                &native_guard,
            )
            .await
            .unwrap_err(),
        Error::StaleContext
    );
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
