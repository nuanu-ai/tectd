//! Candidate context retains baseline-large Programs and fragments exact source text.
#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::commit_create;
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tect_application::{CandidateGuidance, CandidateOutputGuard, WorkspaceService};
use tect_domain::{
    BeginCandidateSet, CandidateMethodSnapshot, CandidateRuleSnapshot, CandidateSnapshotMaterial,
    Error, Program, RequestContext, ResolvedCandidateDraft, Result, StoredCandidateContext,
    WorktreeSummary,
};
use tect_host::{CandidateEncoding, GitSourceInspector, LocalSetupFiles};
use tect_postgres::PgStore;
use tect_postgres::admin;
use uuid::Uuid;

const LEGACY_NAME_BYTES: usize = 5_507_620;

fn id(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap()).unwrap()
}

async fn reconstruct(
    client: &mut Mcp,
    set: Uuid,
    source: Uuid,
    draft_revision: Option<i64>,
) -> (String, usize) {
    let mut cursor = 0_u64;
    let mut result = String::new();
    let mut pages = 0;
    loop {
        let mut params = json!({"candidate_set_id":set,"view":"fragment","source_ref_id":source,"cursor":cursor});
        if let Some(revision) = draft_revision {
            params["draft_revision"] = json!(revision);
        }
        let page = client.call("candidate_context", params).await;
        assert!(serde_json::to_vec(&page).unwrap().len() < 8 * 1024 * 1024);
        let fragment = &page["fragment"];
        assert_eq!(fragment["cursor"], cursor);
        let part = fragment["text"].as_str().unwrap();
        assert!(!part.is_empty());
        result.push_str(part);
        pages += 1;
        let Some(next) = fragment["next_cursor"].as_u64() else {
            break;
        };
        assert!(next > cursor);
        cursor = next;
    }
    (result, pages)
}

async fn canonical(pool: &PgPool, set: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT table_name||':'||row_value FROM (\
           SELECT 'scope_candidate_sets' table_name,xmin::text||':'||row_to_json(s)::text row_value \
             FROM scope_candidate_sets s WHERE id=$1 \
           UNION ALL SELECT 'scope_candidate_contents',xmin::text||':'||row_to_json(c)::text \
             FROM scope_candidate_contents c WHERE workspace_id=(SELECT workspace_id FROM scope_candidate_sets WHERE id=$1) \
           UNION ALL SELECT 'scope_candidate_source_refs',xmin::text||':'||row_to_json(r)::text \
             FROM scope_candidate_source_refs r WHERE candidate_set_id=$1 \
           UNION ALL SELECT 'scope_candidate_drafts',xmin::text||':'||row_to_json(d)::text \
             FROM scope_candidate_drafts d WHERE candidate_set_id=$1 \
           UNION ALL SELECT 'scope_candidate_receipts',xmin::text||':'||row_to_json(x)::text \
             FROM scope_candidate_receipts x WHERE candidate_set_id=$1\
         ) q ORDER BY table_name,row_value",
    )
    .bind(set)
    .fetch_all(pool)
    .await
    .unwrap()
}

struct FixtureGuidance;

impl CandidateGuidance for FixtureGuidance {
    fn snapshot(
        &self,
        program: Program,
        selected_worktrees: Vec<WorktreeSummary>,
    ) -> Result<CandidateSnapshotMaterial> {
        let body = "Test-only output guard fixture.";
        Ok(CandidateSnapshotMaterial {
            program,
            selected_worktrees,
            selected_sources_digest: "0".repeat(64),
            method: CandidateMethodSnapshot {
                id: "guard-fixture".into(),
                revision: "1".into(),
                digest: "53d64e57029f3ab7c5b4d83e1a9738b07813e11fd2881e0a1d6aa6ca2955d9d0".into(),
                body: body.into(),
                origin_refs: vec!["test:scope_candidate_capacity".into()],
            },
            registry_revision: "1".into(),
            registry_digest: "2".repeat(64),
            rules: vec![CandidateRuleSnapshot {
                id: "guard-rule".into(),
                revision: "1".into(),
                text: "Exercise the actual host encoder after insertion.".into(),
                origin_refs: vec!["test:scope_candidate_capacity".into()],
                applicability: vec!["purpose=guard-test".into()],
            }],
        })
    }
}

struct BeginRejectingGuard {
    material: CandidateEncoding,
    begin: CandidateEncoding,
    begin_checked: AtomicBool,
}

impl CandidateOutputGuard for BeginRejectingGuard {
    fn input_bytes(&self, input: &str) -> Result<i64> {
        self.material.input_bytes(input)
    }
    fn check_material(&self, material: &CandidateSnapshotMaterial) -> Result<()> {
        self.material.check_material(material)
    }
    fn check_draft(&self, draft: &ResolvedCandidateDraft) -> Result<()> {
        self.material.check_draft(draft)
    }
    fn check_stored(&self, stored: &StoredCandidateContext) -> Result<()> {
        self.material.check_stored(stored)
    }
    fn check_begin(&self, outcome: &tect_domain::BeginCandidateSetOutcome) -> Result<()> {
        self.begin_checked.store(true, Ordering::SeqCst);
        self.begin.check_begin(outcome)
    }
}

async fn candidate_rows_for_program(
    pool: &PgPool,
    program: Uuid,
) -> (i64, i64, i64, i64, i64, i64) {
    sqlx::query_as(
        "SELECT
          (SELECT count(*) FROM scope_candidate_sets WHERE program_id=$1),
          (SELECT count(*) FROM scope_candidate_inputs i JOIN scope_candidate_sets s ON s.id=i.candidate_set_id WHERE s.program_id=$1),
          (SELECT count(*) FROM scope_candidate_snapshots n JOIN scope_candidate_sets s ON s.id=n.candidate_set_id WHERE s.program_id=$1),
          (SELECT count(*) FROM scope_candidate_source_refs r JOIN scope_candidate_sets s ON s.id=r.candidate_set_id WHERE s.program_id=$1),
          (SELECT count(*) FROM scope_candidate_contents c WHERE c.workspace_id=(SELECT workspace_id FROM programs WHERE id=$1)),
          (SELECT count(*) FROM scope_candidate_receipts x JOIN scope_candidate_sets s ON s.id=x.candidate_set_id WHERE s.program_id=$1)",
    )
    .bind(program)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn baseline_large_program_and_input_are_exactly_fragmented_and_failed_output_rolls_back() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("scope-capacity.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-scope-capacity-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace = format!("scope-capacity-{}", Uuid::new_v4().simple());
    let native = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config, &native, &workspace).await;
    client.call("open_workspace", json!({})).await;

    let program = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"Create the large captured Program fixture."}),
        )
        .await;
    let program_id = id(&program["program"]["id"]);
    let name = "P".repeat(LEGACY_NAME_BYTES);
    sqlx::query(
        "UPDATE programs SET revision=2,status='open',current_step='ready',input_cursor=1,\
         name=$2,intent='Prove bounded candidate context reads',basis='Captured input',\
         boundaries='Owned test fixture only',constraints='No truncation',\
         success='Every byte is reconstructed' WHERE id=$1",
    )
    .bind(program_id)
    .bind(&name)
    .execute(&pool)
    .await
    .unwrap();

    let direct = WorkspaceService::new(
        Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap()),
        Arc::new(GitSourceInspector),
        Arc::new(LocalSetupFiles),
    );
    let guard = BeginRejectingGuard {
        material: CandidateEncoding {
            capacity: 8 * 1024 * 1024,
        },
        begin: CandidateEncoding { capacity: 128 },
        begin_checked: AtomicBool::new(false),
    };
    let refused_begin = direct
        .begin_candidate_set(
            &RequestContext {
                auth: enrollment.auth.clone(),
                native_session_id: native.clone(),
                workspace_key: workspace.clone(),
            },
            &BeginCandidateSet {
                request_id: Uuid::new_v4(),
                program_id,
                program_revision: 2,
                boundary: tect_domain::CandidateBoundary::Ongoing,
                input: "This material fits; only the actual begin response exceeds the injected budget."
                    .into(),
                task_context: Default::default(),
                advisory_preference: Default::default(),
            },
            &FixtureGuidance,
            &guard,
        )
        .await;
    assert_eq!(refused_begin, Err(Error::RequestTooLarge));
    assert!(guard.begin_checked.load(Ordering::SeqCst));
    assert_eq!(
        candidate_rows_for_program(&pool, program_id).await,
        (0, 0, 0, 0, 0, 0)
    );

    let mut capacity_document: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/planning-abstraction.json"
    ))
    .unwrap();
    let large_instruction = "K".repeat(7_000);
    capacity_document["document"]["planning_briefs"] = Value::Array(
        (0..64)
            .map(|index| {
                json!({
                    "local_id":format!("capacity-scope-{index}"),"stage":"scope",
                    "instruction":large_instruction,"conditions":[],"exceptions":[],
                    "purpose":"Capacity rollback proof.",
                    "selectors":{"target_iris":["urn:tect:dk4:capacity:match"]}
                })
            })
            .collect(),
    );
    commit_create(&mut client, capacity_document["document"].clone()).await;
    let before_large_manifest = candidate_rows_for_program(&pool, program_id).await;
    let large_manifest_guard = BeginRejectingGuard {
        material: CandidateEncoding {
            capacity: 8 * 1024 * 1024,
        },
        begin: CandidateEncoding { capacity: 128 },
        begin_checked: AtomicBool::new(false),
    };
    let large_manifest_refused = direct
        .begin_candidate_set(
            &RequestContext {
                auth: enrollment.auth.clone(),
                native_session_id: native.clone(),
                workspace_key: workspace.clone(),
            },
            &BeginCandidateSet {
                request_id: Uuid::new_v4(),
                program_id,
                program_revision: 2,
                boundary: tect_domain::CandidateBoundary::Ongoing,
                input: "Deliver every applicable capacity brief without truncation.".into(),
                task_context: tect_domain::PlanningTaskContext {
                    target_iris: Some(vec!["urn:tect:dk4:capacity:match".into()]),
                    ..Default::default()
                },
                advisory_preference: Default::default(),
            },
            &FixtureGuidance,
            &large_manifest_guard,
        )
        .await;
    assert!(matches!(
        large_manifest_refused,
        Err(Error::RequestTooLarge)
    ));
    assert!(large_manifest_guard.begin_checked.load(Ordering::SeqCst));
    assert_eq!(
        candidate_rows_for_program(&pool, program_id).await,
        before_large_manifest
    );

    let input = format!(
        "{}{}",
        "\\\"escaped\\n".repeat(75_000),
        "界🧪".repeat(75_000)
    );
    let created = client
        .call(
            "begin_candidate_set",
            json!({"request_id":Uuid::new_v4(),"program_id":program_id,"program_revision":2,
                "boundary":"ongoing","input":input,
                "task_context":{"target_iris":["urn:tect:dk4:capacity:other"]}}),
        )
        .await;
    let set = id(&created["context"]["candidate_set"]["id"]);
    let before_reads = canonical(&pool, set).await;

    let program_page = client
        .call(
            "candidate_context",
            json!({"candidate_set_id":set,"view":"program","limit":25}),
        )
        .await;
    let name_ref = program_page["program"]["field_refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["program_field"] == "name")
        .map(|value| id(&value["id"]))
        .unwrap();
    let (read_name, name_pages) = reconstruct(&mut client, set, name_ref, None).await;
    assert_eq!(read_name, name);
    assert!(name_pages > 20);
    let inputs = client
        .call(
            "candidate_context",
            json!({"candidate_set_id":set,"view":"inputs","limit":25}),
        )
        .await;
    let input_ref = id(&inputs["items"][0]["input"]["source_ref_id"]);
    let (read_input, input_pages) = reconstruct(&mut client, set, input_ref, None).await;
    assert_eq!(read_input, input);
    assert!(input_pages > 2);
    assert_eq!(canonical(&pool, set).await, before_reads);

    let too_large = "\"".repeat(2_500_000);
    let refused = client
        .call_error(
            "save_candidate_set",
            json!({"kind":"draft","candidate_set_id":set,"revision":1,
                "snapshot_id":id(&created["context"]["snapshot"]["id"]),"input_cursor":1,
                "request_id":Uuid::new_v4(),"draft":{"boundary":"ongoing","goals":[{
                    "identity":{"local":"goal"},"text":"Read the request","source_ref_id":input_ref,
                    "resolution":{"kind":"candidate","reference":{"local":"candidate"}}}],
                    "evidence":[],"candidates":[{"identity":{"local":"candidate"},
                    "title":"Oversized result","outcome":"Must roll back","trigger":"Save",
                    "delivered_behavior":too_large,"proof":"No row persists","coverage_goals":[{"local":"goal"}]}],
                    "blockers":[]}}),
        )
        .await;
    assert_eq!(refused["error"]["code"], "request_too_large");
    assert_eq!(canonical(&pool, set).await, before_reads);

    let saved = client.call("save_candidate_set", json!({
        "kind":"draft","candidate_set_id":set,"revision":1,
        "snapshot_id":id(&created["context"]["snapshot"]["id"]),"input_cursor":1,
        "request_id":Uuid::new_v4(),"draft":{"boundary":"ongoing","goals":[{
            "identity":{"local":"goal"},"text":"Read the request","source_ref_id":input_ref,
            "resolution":{"kind":"candidate","reference":{"local":"candidate"}}}],
            "evidence":[],"candidates":[{"identity":{"local":"candidate"},
            "title":"Bounded result","outcome":"Historical context remains readable","trigger":"Query history",
            "delivered_behavior":"Read exact retained sources","proof":"Reconstruct every fragment",
            "coverage_goals":[{"local":"goal"}]}],"blockers":[]}}
    )).await;
    assert_eq!(saved["context"]["candidate_set"]["revision"], 2);
    let before_historical_reads = canonical(&pool, set).await;
    let historical = client
        .call(
            "candidate_context",
            json!({
                "candidate_set_id":set,"view":"historical","draft_revision":2,"limit":25
            }),
        )
        .await;
    assert!(serde_json::to_vec(&historical).unwrap().len() < 8 * 1024 * 1024);
    assert_eq!(
        historical["historical"]["snapshot"]["id"],
        created["context"]["snapshot"]["id"]
    );
    let (historical_name, historical_pages) =
        reconstruct(&mut client, set, name_ref, Some(2)).await;
    assert_eq!(historical_name, name);
    assert!(historical_pages > 20);
    assert_eq!(canonical(&pool, set).await, before_historical_reads);

    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
