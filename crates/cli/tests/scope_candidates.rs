//! Real PostgreSQL/daemon/stdio candidate planning, replay, restart, and bounded reads.
#[path = "scope_candidates/actions.rs"]
mod actions;
#[path = "scope_candidates/covered.rs"]
mod covered;
mod recovery_support;

use actions::{candidate_action, id, rows};
use recovery_support::{
    Daemon, Mcp, action_name, action_params, host_file, private_temp, tagged_url,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tect_postgres::admin;
use uuid::Uuid;

fn planning_ref(context: &Value, sequence: i64) -> Uuid {
    context["snapshot"]["source_refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["kind"] == "planning_input" && value["input_sequence"] == sequence)
        .map(|value| id(&value["id"]))
        .unwrap()
}

fn success_ref(context: &Value) -> Uuid {
    context["snapshot"]["source_refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["kind"] == "program_success")
        .map(|value| id(&value["id"]))
        .unwrap()
}

async fn text(client: &mut Mcp, set: Uuid, source: Uuid) -> String {
    let mut cursor = 0_u64;
    let mut result = String::new();
    loop {
        let page = client
            .call(
                "candidate_context",
                json!({"candidate_set_id":set,"view":"fragment","source_ref_id":source,"cursor":cursor}),
            )
            .await;
        let fragment = &page["fragment"];
        assert_eq!(fragment["cursor"], cursor);
        let part = fragment["text"].as_str().unwrap();
        assert!(!part.is_empty() || fragment["next_cursor"].is_null());
        result.push_str(part);
        let Some(next) = fragment["next_cursor"].as_u64() else {
            break;
        };
        assert!(next > cursor);
        cursor = next;
    }
    result
}

async fn create_program(client: &mut Mcp, input: &str, name: &str) -> Uuid {
    let created = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":input}),
        )
        .await;
    let program = id(&created["program"]["id"]);
    let saved = client
        .call(
            "save_program",
            json!({
                "program_id":program,"revision":1,"input_cursor":1,"name":name,
                "intent":"Inspect \"email\" notification preferences 🧭",
                "basis":"The captured request\nand accepted work",
                "boundaries":"Email only; exclude SMS, push, and analytics",
                "constraints":"Read only; preserve accepted work and \\slashes",
                "success":"Users can inspect email preferences and tests pass",
                "complete":true
            }),
        )
        .await;
    assert_eq!(saved["program"]["status"], "open");
    program
}

fn draft(
    boundary: &str,
    goal: Value,
    evidence: Vec<Value>,
    candidate: Value,
    protected_changes: Vec<Value>,
) -> Value {
    json!({
        "boundary":boundary,
        "goals":[goal],"evidence":evidence,"candidates":[candidate],"blockers":[],
        "protected_changes":protected_changes
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn candidate_set_replans_with_exact_receipts_protected_work_and_fragments() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("scope-candidates.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-scope-{}", Uuid::new_v4()));
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace = format!("scope-candidates-{}", Uuid::new_v4().simple());
    let mut first = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &workspace).await;
    let mut second = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &workspace).await;
    first.call("open_workspace", json!({})).await;
    second.call("open_workspace", json!({})).await;

    let program_input = "Plan email preferences; the existing delivery adapter is accepted work.";
    let program = create_program(&mut first, program_input, "Email preferences").await;
    let original = format!(
        "Build read-only email preferences first. Accepted: reuse delivery adapter. {}🧪",
        "escaped \\\"line\\n".repeat(32_000)
    );
    let begin_request = Uuid::new_v4();
    let created = first
        .call(
            "begin_candidate_set",
            json!({
                "request_id":begin_request,"program_id":program,"program_revision":2,
                "boundary":"ongoing","input":original
            }),
        )
        .await;
    let set = id(&created["context"]["candidate_set"]["id"]);
    assert_eq!(
        action_name(&created["actions"][0]),
        Some("scope.candidates.context")
    );
    assert_eq!(rows(&pool, set).await, (1, 1, 1, 0, 0, 0));
    let context = &created["context"];
    assert_eq!(context["snapshot"]["rules"].as_array().unwrap().len(), 4);
    assert!(
        context["snapshot"]["method"]["body"]
            .as_str()
            .unwrap()
            .contains("Review")
    );

    let program_page = first
        .call(
            "candidate_context",
            json!({"candidate_set_id":set,"view":"program","limit":25}),
        )
        .await;
    assert_eq!(program_page["program"]["id"], program.to_string());
    assert!(program_page["program"].get("name").is_none());
    let refs = program_page["program"]["field_refs"].as_array().unwrap();
    assert_eq!(refs.len(), 6);
    let expected_fields = [
        "Email preferences",
        "Inspect \"email\" notification preferences 🧭",
        "The captured request\nand accepted work",
        "Email only; exclude SMS, push, and analytics",
        "Read only; preserve accepted work and \\slashes",
        "Users can inspect email preferences and tests pass",
    ];
    for (index, source) in refs.iter().enumerate() {
        let part = first
            .call(
                "candidate_context",
                json!({"candidate_set_id":set,"view":"fragment","source_ref_id":source["id"],"cursor":0}),
            )
            .await;
        assert!(part["fragment"]["next_cursor"].is_null());
        assert_eq!(part["fragment"]["text"], expected_fields[index]);
        let next = action_params(&part["actions"][0]);
        if let Some(expected) = refs.get(index + 1) {
            assert_eq!(next["source_ref_id"], expected["id"]);
            assert_eq!(next["view"], "fragment");
        } else {
            assert_eq!(next["view"], "inputs");
        }
    }
    let success = refs
        .iter()
        .find(|value| value["program_field"] == "success")
        .unwrap();
    assert_eq!(
        text(&mut first, set, id(&success["id"])).await,
        "Users can inspect email preferences and tests pass"
    );
    let inputs = first
        .call(
            "candidate_context",
            json!({"candidate_set_id":set,"view":"inputs","limit":25}),
        )
        .await;
    let input_ref = id(&inputs["items"][0]["input"]["source_ref_id"]);
    assert_eq!(text(&mut first, set, input_ref).await, original);

    let accepted_local = json!({
        "identity":{"local":"accepted_adapter"},"kind":"accepted_work",
        "summary":"Reuse the already accepted delivery adapter", "source_ref_id":input_ref,
        "authority_input_sequence":1
    });
    let goal_local = json!({
        "identity":{"local":"email_goal"},"text":"Deliver read-only email preference inspection",
        "source_ref_id":input_ref,"resolution":{"kind":"candidate","reference":{"local":"email_ui"}}
    });
    let candidate_local = json!({
        "identity":{"local":"email_ui"},"title":"Email preference controls",
        "outcome":"Users inspect email notification preferences","trigger":"Open notification settings",
        "delivered_behavior":"Read email preferences without changing them","proof":"Existing read-only integration tests pass",
        "includes":["Read API and UI"],"excludes":["writes","SMS","push"],"dependencies":[],
        "coverage_goals":[{"local":"email_goal"}],"evidence":[{"local":"accepted_adapter"}]
    });
    let save_id = Uuid::new_v4();
    let save = json!({
        "kind":"draft","candidate_set_id":set,"revision":1,
        "snapshot_id":id(&context["snapshot"]["id"]),"input_cursor":1,"request_id":save_id,
            "draft":draft("ongoing", goal_local, vec![accepted_local], candidate_local, vec![])
    });
    let (saved_a, saved_b) = tokio::join!(
        first.call("save_candidate_set", save.clone()),
        second.call("save_candidate_set", save.clone())
    );
    assert_eq!(saved_a, saved_b);
    assert_eq!(saved_a["context"]["candidate_set"]["revision"], 2);
    assert_eq!(rows(&pool, set).await, (1, 1, 1, 1, 0, 1));
    let candidate_id = id(&saved_a["draft"]["candidates"][0]["id"]);
    let goal_id = id(&saved_a["draft"]["goals"][0]["id"]);
    let accepted_id = id(&saved_a["draft"]["evidence"][0]["id"]);

    let reviewed = first.call("save_candidate_set", json!({
        "kind":"review","candidate_set_id":set,"revision":2,
        "snapshot_id":id(&context["snapshot"]["id"]),"input_cursor":1,"request_id":Uuid::new_v4(),
        "review":{"verdict":"ready","summary":"The candidate is vertical and the accepted work is traceable.",
            "findings":[],"candidate_decisions":[{"candidate_id":candidate_id,"decision":"accept","rationale":"Bounded and provable"}]}
    })).await;
    assert_eq!(reviewed["context"]["candidate_set"]["status"], "ready");
    assert_eq!(reviewed["recommended_action"], 0);
    assert_eq!(
        action_name(&reviewed["actions"][0]),
        Some("scope.candidates.context")
    );
    let record_action = &reviewed["actions"][1];
    let mut record_params =
        candidate_action(record_action, "scope.candidates.record_input", None, set, 3);
    let record_request_id = id(&record_params["request_id"]);

    let amendment =
        "Do not reuse that adapter; replace the accepted-work link after this authorization.";
    record_params["input"] = json!(amendment);
    let recorded = first.call("record_candidate_input", record_params).await;
    assert_eq!(recorded["context"]["candidate_set"]["revision"], 4);
    assert_eq!(recorded["context"]["candidate_set"]["latest_input"], 2);
    assert_eq!(
        recorded["context"]["candidate_set"]["status"],
        "review_required"
    );
    assert_eq!(
        recorded["context"]["candidate_set"]["revision"],
        reviewed["context"]["candidate_set"]["revision"]
            .as_i64()
            .unwrap()
            + 1
    );
    assert_eq!(
        action_name(&recorded["actions"][0]),
        Some("scope.candidates.refresh")
    );
    let refresh_params = action_params(&recorded["actions"][0]).clone();
    let refreshed = first.call("refresh_candidate_set", refresh_params).await;
    let current = &refreshed["context"];
    let current_original = planning_ref(current, 1);
    let authority = planning_ref(current, 2);
    assert_ne!(
        current_original, input_ref,
        "refresh must create a new snapshot-local ref"
    );
    assert_eq!(text(&mut first, set, current_original).await, original);
    assert_eq!(text(&mut first, set, authority).await, amendment);

    let candidates_page = first
        .call(
            "candidate_context",
            action_params(&refreshed["actions"][0]).clone(),
        )
        .await;
    let reviews_page = first
        .call(
            "candidate_context",
            action_params(&candidates_page["actions"][0]).clone(),
        )
        .await;
    assert_eq!(reviews_page["recommended_action"], 0);
    assert_eq!(reviews_page["actions"].as_array().unwrap().len(), 2);
    let offered_review = candidate_action(
        &reviews_page["actions"][0],
        "scope.candidates.save",
        Some("review"),
        set,
        5,
    );
    let offered_draft = candidate_action(
        &reviews_page["actions"][1],
        "scope.candidates.save",
        Some("draft"),
        set,
        5,
    );
    assert_eq!(offered_review["snapshot_id"], current["snapshot"]["id"]);
    assert_eq!(offered_draft["snapshot_id"], current["snapshot"]["id"]);
    assert_eq!(offered_review["input_cursor"], 2);
    assert_eq!(offered_draft["input_cursor"], 2);
    assert_ne!(offered_review["request_id"], offered_draft["request_id"]);
    assert_ne!(id(&offered_draft["request_id"]), record_request_id);
    let mut remapped_params = offered_draft;
    remapped_params["draft"] = draft(
        "ongoing",
        json!({"identity":{"id":goal_id,"revision":1},"text":"Deliver read-only email preference inspection",
                "source_ref_id":current_original,"resolution":{"kind":"candidate","reference":{"id":candidate_id}}}),
        vec![
            json!({"identity":{"id":accepted_id,"revision":1},"kind":"accepted_work",
                "summary":"Reuse the already accepted delivery adapter","source_ref_id":current_original,
                "authority_input_sequence":1}),
        ],
        json!({"identity":{"id":candidate_id,"revision":1},"title":"Email preference controls",
                "outcome":"Users inspect email notification preferences","trigger":"Open notification settings",
                "delivered_behavior":"Read email preferences without changing them","proof":"Existing read-only integration tests pass",
                "includes":["Read API and UI"],"excludes":["writes","SMS","push"],"dependencies":[],
                "coverage_goals":[{"id":goal_id}],"evidence":[{"id":accepted_id}]}),
        vec![],
    );
    let remapped = first.call("save_candidate_set", remapped_params).await;
    assert_eq!(remapped["context"]["candidate_set"]["revision"], 6);
    assert_eq!(remapped["draft"]["goals"][0]["revision"], 1);
    assert_eq!(remapped["draft"]["candidates"][0]["revision"], 1);
    assert_eq!(
        remapped["draft"]["evidence"][0]["source_ref_id"],
        current_original.to_string()
    );
    assert!(
        remapped["draft"]["protected_changes"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let goal_existing = json!({
        "identity":{"id":goal_id,"revision":1},"text":"Deliver read-only email preference inspection",
        "source_ref_id":authority,"resolution":{"kind":"candidate","reference":{"id":candidate_id}}
    });
    let candidate_existing = json!({
        "identity":{"id":candidate_id,"revision":1},"change_rationale":"The later amendment withdraws the old accepted adapter",
        "title":"Email preference controls",
        "outcome":"Users inspect email notification preferences","trigger":"Open notification settings",
        "delivered_behavior":"Read email preferences without the old adapter",
        "proof":"Existing read-only integration tests pass","includes":["Read API and UI"],"excludes":["writes","SMS","push"],
        "dependencies":[],"coverage_goals":[{"id":goal_id}],"evidence":[]
    });
    let before_forbidden = rows(&pool, set).await;
    let mut candidate_with_evidence = candidate_existing.clone();
    candidate_with_evidence["evidence"] = json!([{"id":accepted_id}]);
    let evidence_only = first.call_error("save_candidate_set", json!({
        "kind":"draft","candidate_set_id":set,"revision":6,"snapshot_id":id(&current["snapshot"]["id"]),
        "input_cursor":2,"request_id":Uuid::new_v4(),"draft":draft("ongoing",
            goal_existing.clone(),vec![json!({"identity":{"id":accepted_id,"revision":1},
                "kind":"verified_evidence","summary":"Reclassified without authority",
                "source_ref_id":current_original,"authority_input_sequence":1})],
            candidate_with_evidence,vec![])
    })).await;
    assert_eq!(evidence_only["error"]["code"], "forbidden");
    assert_eq!(rows(&pool, set).await, before_forbidden);
    let edge_only = first.call_error("save_candidate_set", json!({
        "kind":"draft","candidate_set_id":set,"revision":6,"snapshot_id":id(&current["snapshot"]["id"]),
        "input_cursor":2,"request_id":Uuid::new_v4(),"draft":draft("ongoing",
            goal_existing.clone(),vec![json!({"identity":{"id":accepted_id,"revision":1},
                "kind":"accepted_work","summary":"Reuse the already accepted delivery adapter",
                "source_ref_id":current_original,"authority_input_sequence":1})],
            candidate_existing.clone(),vec![])
    })).await;
    assert_eq!(edge_only["error"]["code"], "forbidden");
    assert_eq!(rows(&pool, set).await, before_forbidden);
    let forbidden = first.call_error("save_candidate_set", json!({
        "kind":"draft","candidate_set_id":set,"revision":6,"snapshot_id":id(&current["snapshot"]["id"]),
        "input_cursor":2,"request_id":Uuid::new_v4(),
        "draft":draft("ongoing",goal_existing.clone(),vec![],candidate_existing.clone(),vec![])
    })).await;
    assert_eq!(forbidden["error"]["code"], "forbidden");
    assert_eq!(rows(&pool, set).await, before_forbidden);

    let changed = first.call("save_candidate_set", json!({
        "kind":"draft","candidate_set_id":set,"revision":6,"snapshot_id":id(&current["snapshot"]["id"]),
        "input_cursor":2,"request_id":Uuid::new_v4(),
        "draft":draft("ongoing",goal_existing,vec![],candidate_existing,vec![json!({
            "accepted_evidence_id":accepted_id,"disposition":"delete",
            "rationale":"The later captured amendment explicitly withdraws the evidence", "authority_source_ref_id":authority
        }),json!({
            "accepted_evidence_id":accepted_id,"prior_candidate_id":candidate_id,"disposition":"delete",
            "rationale":"The later captured amendment explicitly withdraws the candidate link", "authority_source_ref_id":authority
        })])
    })).await;
    assert_eq!(changed["context"]["candidate_set"]["revision"], 7);
    assert_eq!(
        changed["draft"]["protected_changes"][0]["accepted_evidence_id"],
        accepted_id.to_string()
    );
    let review_page = first
        .call(
            "candidate_context",
            json!({"candidate_set_id":set,"view":"reviews","limit":25}),
        )
        .await;
    let offered = action_params(&review_page["actions"][0]);
    assert_eq!(offered["kind"], "review");
    assert_eq!(
        offered["review"]["protected_change_reviews"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(offered["request_id"].as_str().is_some());
    let mut finished_params = offered.clone();
    finished_params["review"] = json!({
        "verdict":"ready","summary":"The later authority text supports removing the old accepted-work link.",
        "findings":[],"candidate_decisions":[{"candidate_id":candidate_id,"decision":"accept","rationale":"Updated result remains vertical"}],
        "protected_change_reviews":[{"accepted_evidence_id":accepted_id,
            "rationale":"Reviewed evidence deletion against the exact second planning input"},
            {"accepted_evidence_id":accepted_id,"prior_candidate_id":candidate_id,
            "rationale":"Reviewed edge deletion against the exact second planning input"}]
    });
    let finished = first.call("save_candidate_set", finished_params).await;
    assert_eq!(finished["context"]["candidate_set"]["status"], "ready");
    assert_eq!(finished["recommended_action"], 0);
    assert_eq!(finished["actions"].as_array().unwrap().len(), 2);
    assert_eq!(
        action_name(&finished["actions"][0]),
        Some("scope.candidates.context")
    );
    assert_eq!(
        action_name(&finished["actions"][1]),
        Some("scope.candidates.record_input")
    );
    let terminal = first
        .call(
            "candidate_context",
            json!({"candidate_set_id":set,"view":"reviews","limit":25}),
        )
        .await;
    assert!(terminal["recommended_action"].is_null());
    assert_eq!(terminal["actions"].as_array().unwrap().len(), 1);
    assert_eq!(
        action_name(&terminal["actions"][0]),
        Some("scope.candidates.record_input")
    );
    assert!(
        terminal["terminal_note"]
            .as_str()
            .unwrap()
            .contains("Native Scope opening is not available")
    );

    let before_replay = rows(&pool, set).await;
    let delayed = second.call("save_candidate_set", save.clone()).await;
    assert_eq!(delayed, saved_a);
    assert_eq!(delayed["context"]["candidate_set"]["revision"], 2);
    assert_eq!(rows(&pool, set).await, before_replay);
    let conflict = second
        .call_error("save_candidate_set", {
            let mut changed = save;
            changed["draft"]["candidates"][0]["title"] = json!("Conflicting retry");
            changed
        })
        .await;
    assert_eq!(conflict["error"]["code"], "input_conflict");
    assert_eq!(rows(&pool, set).await, before_replay);

    let state = first.call("get_state", json!({})).await;
    assert_eq!(state["candidate_sets"][0]["id"], set.to_string());
    assert_eq!(
        action_name(&state["actions"][0]),
        Some("scope.candidates.context")
    );
    assert_eq!(action_params(&state["actions"][0])["view"], "overview");

    covered::run(&mut first, &pool).await;

    first.finish().await;
    second.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
