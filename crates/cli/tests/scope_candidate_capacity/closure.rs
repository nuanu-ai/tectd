use super::*;
use sha2::{Digest, Sha256};
use tect_domain::{CandidateSnapshot, SaveCandidateDraft};

pub(super) async fn all_rows(pool: &PgPool) -> Vec<(String, Vec<String>)> {
    let names: Vec<String> = sqlx::query_scalar("SELECT tablename FROM pg_tables WHERE schemaname='public' AND (tablename LIKE 'scope_candidate_%' OR tablename LIKE 'planning_knowledge_%' OR tablename IN ('native_planning_receipts','knowledge_owned_copies','knowledge_maintenance_consumers')) ORDER BY tablename")
        .fetch_all(pool).await.unwrap();
    assert!(names.contains(&"scope_candidate_drafts".into()));
    assert!(names.contains(&"planning_knowledge_consumptions".into()));
    let mut result = Vec::new();
    for name in names {
        assert!(
            name.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        );
        let rows = sqlx::query_scalar(&format!("SELECT xmin::text||':'||row_to_json(t)::text FROM \"{name}\" t ORDER BY row_to_json(t)::text"))
            .fetch_all(pool).await.unwrap();
        result.push((name, rows));
    }
    result
}

struct CapturedGuidance(CandidateSnapshot);
impl CandidateGuidance for CapturedGuidance {
    fn snapshot(
        &self,
        program: Program,
        selected_worktrees: Vec<WorktreeSummary>,
    ) -> Result<CandidateSnapshotMaterial> {
        assert!(selected_worktrees.is_empty());
        Ok(CandidateSnapshotMaterial {
            program,
            selected_worktrees,
            selected_sources_digest: self.0.selected_sources_digest.clone(),
            method: self.0.method.clone(),
            registry_revision: self.0.registry_revision.clone(),
            registry_digest: self.0.registry_digest.clone(),
            rules: self.0.rules.clone(),
        })
    }
}
struct SaveRejectingGuard {
    checked: AtomicBool,
}
impl CandidateOutputGuard for SaveRejectingGuard {
    fn input_bytes(&self, input: &str) -> Result<i64> {
        CandidateEncoding { capacity: 8192 }.input_bytes(input)
    }
    fn check_material(&self, material: &CandidateSnapshotMaterial) -> Result<()> {
        CandidateEncoding { capacity: 8192 }.check_material(material)
    }
    fn check_draft(&self, draft: &ResolvedCandidateDraft) -> Result<()> {
        CandidateEncoding { capacity: 8192 }.check_draft(draft)
    }
    fn check_begin(&self, outcome: &tect_domain::BeginCandidateSetOutcome) -> Result<()> {
        CandidateEncoding { capacity: 8192 }.check_begin(outcome)
    }
    fn check_stored(&self, stored: &StoredCandidateContext) -> Result<()> {
        assert_eq!(stored.context.candidate_set.revision, 2);
        let draft = stored.draft.as_ref().unwrap();
        assert_eq!(draft.candidates.len(), 1);
        assert_eq!(draft.goals.len(), 1);
        // The service invokes this guard after save_candidate_draft and receipt SQL.
        self.checked.store(true, Ordering::SeqCst);
        CandidateEncoding { capacity: 128 }.check_stored(stored)
    }
}

pub(super) fn draft(set: Uuid, snapshot: Uuid, source: Uuid, behavior: &str) -> Value {
    json!({"kind":"draft","candidate_set_id":set,"revision":1,"snapshot_id":snapshot,"input_cursor":1,
        "request_id":Uuid::new_v4(),"draft":{"boundary":"ongoing","goals":[{
            "identity":{"local":"goal"},"text":"Read the request","source_ref_id":source,
            "resolution":{"kind":"candidate","reference":{"local":"candidate"}}}],
            "evidence":[],"candidates":[{"identity":{"local":"candidate"},
            "title":"Bounded result","outcome":"Historical context remains readable","trigger":"Query history",
            "delivered_behavior":behavior,"proof":"Reconstruct every fragment","coverage_goals":[{"local":"goal"}]}],"blockers":[]}})
}

pub(super) async fn rollback(
    service: &WorkspaceService,
    context: &RequestContext,
    pool: &PgPool,
    overview: &Value,
    source: Uuid,
) {
    let candidate = &overview["context"];
    let before = all_rows(pool).await;
    let mut arguments = draft(
        id(&candidate["candidate_set"]["id"]),
        id(&candidate["snapshot"]["id"]),
        source,
        "Read exact retained sources",
    );
    arguments.as_object_mut().unwrap().remove("kind");
    let request: SaveCandidateDraft = serde_json::from_value(arguments).unwrap();
    let guidance = CapturedGuidance(serde_json::from_value(candidate["snapshot"].clone()).unwrap());
    let guard = SaveRejectingGuard {
        checked: AtomicBool::new(false),
    };
    let result = service
        .save_candidate_draft(context, &request, &guidance, &guard)
        .await;
    assert_eq!(result, Err(Error::RequestTooLarge));
    assert!(guard.checked.load(Ordering::SeqCst));
    assert_eq!(all_rows(pool).await, before);
    eprintln!(
        "save_output_guard_after_sql=true budget=128 rollback_all_candidate_and_planning_tables=true"
    );
}

pub(super) async fn accepted_large(client: &mut Mcp, pool: &PgPool, original_program: Uuid) {
    let program = client.call("begin_program", json!({"request_id":Uuid::new_v4(),"input":"Verify an independent large accepted draft."})).await;
    let program_id = id(&program["program"]["id"]);
    assert_ne!(program_id, original_program);
    client.call("save_program", json!({"program_id":program_id,"revision":1,"input_cursor":1,
        "name":"Large accepted draft","intent":"Read all retained candidate bytes","basis":"Independent fixture",
        "boundaries":"Owned database only","constraints":"No truncation","success":"Exact complete draft read","complete":true})).await;
    let begun = client
        .call(
            "begin_candidate_set",
            json!({"request_id":Uuid::new_v4(),"program_id":program_id,"program_revision":2,
        "boundary":"ongoing","input":"Retain exact quote characters."}),
        )
        .await;
    let begun = CandidateFixture::from_mutation(begun);
    let overview = begun.read_overview(client).await.value;
    let set = id(&overview["context"]["candidate_set"]["id"]);
    let inputs = read_query_json(client, &json!({"route":"scope.candidates.context","params":{"candidate_set_id":set,"view":"inputs","limit":25}})).await;
    let behavior = "\"".repeat(2_500_000);
    let response = client
        .exchange(
            "tools/call",
            recovery_support::public_call(
                "save_candidate_set",
                draft(
                    set,
                    id(&overview["context"]["snapshot"]["id"]),
                    id(&inputs.value["items"][0]["input"]["source_ref_id"]),
                    &behavior,
                ),
            ),
        )
        .await;
    assert!(response.get("error").is_none());
    assert_eq!(response["result"]["isError"], false);
    let wire_bytes = serde_json::to_vec(&response).unwrap().len();
    assert!(wire_bytes <= 8192);
    let saved = recovery_support::tool_payload(&response);
    assert_eq!(saved["candidate_set"]["revision"], 2);
    assert_eq!(saved["disposition"], "saved");
    assert!(saved.get("draft").is_none());
    let before = all_rows(pool).await;
    let details = large_details(client, &saved).await;
    assert_eq!(
        details["draft"]["candidates"][0]["delivered_behavior"],
        behavior
    );
    assert_eq!(all_rows(pool).await, before);
    eprintln!(
        "accepted_large_draft quotes=2500000 actual_compact_mcp_wire_bytes={wire_bytes} readonly_db=true"
    );
}

async fn large_details(client: &mut Mcp, mutation: &Value) -> Value {
    let action = &mutation["field_destinations"]["draft"];
    assert_eq!(action["kind"], "ready_call");
    assert_eq!(action["tool"], "query");
    assert_eq!(action["arguments"]["route"], "scope.candidates.context");
    let initial = action["arguments"]["params"].clone();
    assert_eq!(initial["view"], "details");
    let mut arguments = action["arguments"].clone();
    let mut bytes = Vec::new();
    let mut baseline = None::<(Value, Value, usize)>;
    for pages in 1..=8192 {
        let page = client.call("query", arguments.clone()).await;
        assert!(serde_json::to_vec(&page).unwrap().len() <= 8192);
        assert_eq!(page["kind"], "fragment");
        assert_eq!(page["format"], "json");
        assert_eq!(page["encoding"], "utf-8");
        let total = usize::try_from(page["total_bytes"].as_u64().unwrap()).unwrap();
        if let Some((source, digest, budget)) = &baseline {
            assert_eq!(&page["source"], source);
            assert_eq!(&page["representation_digest"], digest);
            assert_eq!(total, *budget);
        } else {
            assert_eq!(
                page["source"]["candidate_set_id"],
                mutation["candidate_set"]["id"]
            );
            assert_eq!(
                page["source"]["candidate_set_revision"],
                mutation["candidate_set"]["revision"]
            );
            assert_eq!(page["source"]["snapshot_id"], mutation["snapshot"]["id"]);
            assert!(total <= 32 * 1024 * 1024); // Explicit budget for this read only, derived from its advertised size.
            baseline = Some((
                page["source"].clone(),
                page["representation_digest"].clone(),
                total,
            ));
            eprintln!(
                "large_details_first total_bytes={total} source={} digest={}",
                page["source"], page["representation_digest"]
            );
        }
        assert_eq!(page["offset_bytes"].as_u64(), Some(bytes.len() as u64));
        let text = page["text"].as_str().unwrap();
        assert_eq!(page["returned_bytes"].as_u64(), Some(text.len() as u64));
        bytes.extend_from_slice(text.as_bytes());
        assert!(bytes.len() <= total);
        if page["next_offset_bytes"].is_null() {
            assert_eq!(bytes.len(), total);
            let digest = format!("{:x}", Sha256::digest(&bytes));
            assert_eq!(page["representation_digest"], digest);
            assert!(page["actions"].is_array());
            assert!(page.get("recommended_action").is_some());
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                value["context"]["candidate_set"]["revision"],
                mutation["candidate_set"]["revision"]
            );
            assert_eq!(
                value["context"]["snapshot"]["id"],
                mutation["snapshot"]["id"]
            );
            eprintln!("large_details_complete pages={pages} bytes={total} sha256={digest}");
            return value;
        }
        assert!(!text.is_empty());
        assert_eq!(page["next_offset_bytes"].as_u64(), Some(bytes.len() as u64));
        let matching = page["actions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|a| {
                a["kind"] == "ready_call"
                    && a["tool"] == "query"
                    && a["arguments"]["route"] == "scope.candidates.context"
                    && a["arguments"]["params"]["offset_bytes"] == page["next_offset_bytes"]
                    && a["arguments"]["params"]["representation_digest"]
                        == page["representation_digest"]
                    && initial
                        .as_object()
                        .unwrap()
                        .iter()
                        .all(|(k, v)| a["arguments"]["params"].get(k) == Some(v))
            })
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1);
        assert_eq!(
            matching[0]["arguments"]["params"]["candidate_set_revision"],
            mutation["candidate_set"]["revision"]
        );
        arguments = matching[0]["arguments"].clone();
    }
    panic!("large Details finite page budget exceeded");
}
