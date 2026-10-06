//! Durable candidate deltas, historical reads, and daemon restart continuation.
use crate::recovery_support::candidate_reads::read_query_json;
mod recovery_support;
#[path = "scope_candidate_continuation/support.rs"]
mod support;
use recovery_support::candidate_collections::read_collection_query;
use recovery_support::candidate_reads::CandidateFixture;
use recovery_support::{
    Daemon, Mcp, action_name, action_params, host_file, private_temp, tagged_url,
};
use serde_json::json;
use sqlx::PgPool;
use support::{
    candidate, existing_candidate, existing_goal, goal, id, planning_ref, read_text, repository,
    versions,
};
use tect_postgres::admin;
use uuid::Uuid;
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn amendment_delta_history_and_restart_preserve_one_candidate_head() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repository_path = root.join("source");
    repository(&repository_path);
    let socket = root.join("continuation.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-continuation-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace = format!("continuation-{}", Uuid::new_v4().simple());
    let native = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config, &native, &workspace).await;
    client.call("open_workspace", json!({})).await;
    let started = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"Plan account notification controls."}),
        )
        .await;
    let program = id(&started["program"]["id"]);
    client.call("save_program", json!({
        "program_id":program,"revision":1,"input_cursor":1,"name":"Notification controls",
        "intent":"Give users bounded notification controls","basis":"Captured request",
        "boundaries":"Email and account surfaces only","constraints":"Preserve accepted work",
        "success":"Users can inspect preferences, change email cadence, and audit delivery",
        "complete":true
    })).await;
    let original_request = "Plan inspect, cadence, and audit outcomes.";
    let created = client
        .call(
            "begin_candidate_set",
            json!({
                "request_id":Uuid::new_v4(),"program_id":program,"program_revision":2,
                "boundary":"ongoing","input":original_request
            }),
        )
        .await;
    let created_fixture = CandidateFixture::from_mutation(created.clone());
    let created_overview = created_fixture.read_overview(&mut client).await.value;
    let set = id(&created_overview["context"]["candidate_set"]["id"]);
    let snapshot_one = id(&created_overview["context"]["snapshot"]["id"]);
    let source_one = planning_ref(&created_overview["context"], 1);
    let initial_request = json!({
        "kind":"draft","candidate_set_id":set,"revision":1,"snapshot_id":snapshot_one,
        "input_cursor":1,"request_id":Uuid::new_v4(),"draft":{"boundary":"ongoing",
        "goals":[goal("ga","Inspect preferences",source_one,"ca"),
            goal("gb","Change email cadence",source_one,"cb"),
            goal("gc","Audit delivery",source_one,"cc")],"evidence":[],
        "candidates":[candidate("ca","Inspect preferences","Users inspect preferences","ga"),
            candidate("cb","Email cadence","Users change email cadence","gb"),
            candidate("cc","Delivery audit","Users audit delivery","gc")],
        "blockers":[]}}
    );
    let initial = client
        .call("save_candidate_set", initial_request.clone())
        .await;
    let initial_fixture = CandidateFixture::from_mutation(initial.clone());
    let initial_details = initial_fixture.read_details(&mut client).await.value;
    assert_eq!(
        initial_details["draft"]["delta"]["added"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let old_goals = initial_details["draft"]["goals"]
        .as_array()
        .unwrap()
        .clone();
    let old_candidates = initial_details["draft"]["candidates"]
        .as_array()
        .unwrap()
        .clone();
    let a = id(&old_candidates[0]["id"]);
    let b = id(&old_candidates[1]["id"]);
    let c = id(&old_candidates[2]["id"]);
    let amendment = "Keep inspection, change cadence to seven days, replace audit with export.";
    client
        .call(
            "save_candidate_set",
            json!({
                "kind":"review","candidate_set_id":set,"revision":2,"snapshot_id":snapshot_one,
                "input_cursor":1,"request_id":Uuid::new_v4(),"review":{"verdict":"ready",
                "summary":"All three initial outcomes are bounded and independently observable.",
                "findings":[],"candidate_decisions":old_candidates.iter().map(|value| json!({
                    "candidate_id":value["id"],"decision":"accept","rationale":"Bounded result"
                })).collect::<Vec<_>>()}
            }),
        )
        .await;
    let source = client
        .call(
            "register_source",
            json!({"path":repository_path.to_string_lossy()}),
        )
        .await;
    client
        .call("select_worktrees", json!({"worktree_ids":[source["id"]]}))
        .await;
    let program_input = client
        .call(
            "record_program_input",
            json!({
                "program_id":program,"request_id":Uuid::new_v4(),
                "input":"Keep history visible after the cadence amendment."
            }),
        )
        .await;
    assert_eq!(program_input["program"]["revision"], 3);
    let saved_program = client.call("save_program", json!({
        "program_id":program,"revision":3,"input_cursor":2,
        "success":"Users inspect preferences, change a seven-day email cadence, and audit delivery",
        "complete":true
    })).await;
    assert_eq!(saved_program["program"]["revision"], 4);
    client
        .call(
            "record_candidate_input",
            json!({
                "candidate_set_id":set,"revision":3,"request_id":Uuid::new_v4(),
                "input":amendment
            }),
        )
        .await;
    let stale_history = read_collection_query(
        &mut client,
        &serde_json::json!({"route":"scope.candidates.context","params":json!({
            "candidate_set_id":set,"view":"history","limit":100
        })}),
    )
    .await;
    assert!(
        !stale_history.pages[0].value["context"]["stale_reasons"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let stale_reasons = stale_history.pages[0].value["context"]["stale_reasons"]
        .as_array()
        .unwrap();
    assert!(stale_reasons.iter().any(|reason| reason == "program"));
    assert!(
        stale_reasons
            .iter()
            .any(|reason| reason == "selected_sources")
    );
    assert!(
        stale_history
            .terminal()
            .terminal_actions
            .iter()
            .all(|action| action_name(action) != Some("scope.candidates.refresh"))
    );
    let historical_call = stale_history
        .terminal()
        .terminal_actions
        .iter()
        .find(|action| action_params(action)["view"] == "historical")
        .unwrap();
    assert_eq!(action_params(historical_call)["draft_revision"], 2);
    let refreshed = client
        .call(
            "refresh_candidate_set",
            json!({
                "candidate_set_id":set,"revision":4,"request_id":Uuid::new_v4(),"program_revision":4
            }),
        )
        .await;
    let refreshed_fixture = CandidateFixture::from_mutation(refreshed.clone());
    let refreshed_overview = refreshed_fixture.read_overview(&mut client).await.value;
    let current = &refreshed_overview["context"];
    assert!(current["stale_reasons"].as_array().unwrap().is_empty());
    assert_eq!(current["snapshot"]["method"]["revision"], "6");
    assert_eq!(
        current["snapshot"]["selected_worktree_ids"][0],
        source["id"]
    );
    let current_one = planning_ref(current, 1);
    let current_two = planning_ref(current, 2);
    assert_ne!(current_one, source_one);
    let unchanged_a = existing_candidate(&old_candidates[0], None);
    let mut changed_b = existing_candidate(
        &old_candidates[1],
        Some("The amendment changes cadence from configurable to seven days"),
    );
    changed_b["delivered_behavior"] = json!("Users set a seven-day email cadence");
    let changed_goal_b = existing_goal(&old_goals[1], current_one);
    let draft_without_supersession = json!({
        "boundary":"ongoing","goals":[existing_goal(&old_goals[0],current_one),changed_goal_b,
            goal("gd","Export delivery history",current_two,"cd")],"evidence":[],
        "candidates":[unchanged_a.clone(),changed_b.clone(),
            candidate("cd","Delivery export","Users export delivery history","gd")],"blockers":[]
    });
    let before_rejections = versions(&pool, set).await;
    let omitted = client.call_error("save_candidate_set", json!({
        "kind":"draft","candidate_set_id":set,"revision":5,"snapshot_id":current["snapshot"]["id"],
        "input_cursor":2,"request_id":Uuid::new_v4(),"draft":draft_without_supersession
    })).await;
    assert_eq!(omitted["error"]["code"], "invalid_arguments");
    let mut no_rationale = changed_b.clone();
    no_rationale
        .as_object_mut()
        .unwrap()
        .remove("change_rationale");
    let rejected = client.call_error("save_candidate_set", json!({
        "kind":"draft","candidate_set_id":set,"revision":5,"snapshot_id":current["snapshot"]["id"],
        "input_cursor":2,"request_id":Uuid::new_v4(),"draft":{"boundary":"ongoing",
        "goals":[existing_goal(&old_goals[0],current_one),existing_goal(&old_goals[1],current_one),
            goal("gd","Export delivery history",current_two,"cd")],"evidence":[],
        "candidates":[unchanged_a.clone(),no_rationale,
            candidate("cd","Delivery export","Users export delivery history","gd")],"blockers":[],
        "supersessions":[{"candidate_id":c,"revision":1,"reason":"Export replaces audit in this request",
            "replacements":[{"local":"cd"}]}]}}
    )).await;
    assert_eq!(rejected["error"]["code"], "invalid_arguments");
    assert_eq!(versions(&pool, set).await, before_rejections);
    let continued_request = json!({
        "kind":"draft","candidate_set_id":set,"revision":5,"snapshot_id":current["snapshot"]["id"],
        "input_cursor":2,"request_id":Uuid::new_v4(),"draft":{"boundary":"ongoing",
        "goals":[existing_goal(&old_goals[0],current_one),existing_goal(&old_goals[1],current_one),
            goal("gd","Export delivery history",current_two,"cd")],"evidence":[],
        "candidates":[unchanged_a,changed_b,
            candidate("cd","Delivery export","Users export delivery history","gd")],"blockers":[],
        "supersessions":[{"candidate_id":c,"revision":1,"reason":"Export replaces audit in this request",
            "replacements":[{"local":"cd"}]}]}}
    );
    let continued = client.call("save_candidate_set", continued_request).await;
    let continued_fixture = CandidateFixture::from_mutation(continued.clone());
    let continued_overview = continued_fixture.read_overview(&mut client).await.value;
    let continued_details = continued_fixture.read_details(&mut client).await.value;
    assert_eq!(
        continued_overview["context"]["candidate_set"]["revision"],
        6
    );
    assert_eq!(
        continued_details["draft"]["delta"]["unchanged"][0]["candidate_id"],
        a.to_string()
    );
    assert_eq!(
        continued_details["draft"]["delta"]["unchanged"][0]["revision"],
        1
    );
    assert_eq!(
        continued_details["draft"]["delta"]["changed"][0]["candidate_id"],
        b.to_string()
    );
    assert_eq!(
        continued_details["draft"]["delta"]["changed"][0]["from_revision"],
        1
    );
    assert_eq!(
        continued_details["draft"]["delta"]["changed"][0]["to_revision"],
        2
    );
    assert_eq!(
        continued_details["draft"]["delta"]["superseded"][0]["prior"]["id"],
        c.to_string()
    );
    let d = id(&continued_details["draft"]["delta"]["added"][0]["candidate_id"]);
    assert_eq!(
        continued_details["draft"]["delta"]["added"][0]["revision"],
        1
    );
    assert_ne!(d, a);
    assert_eq!(
        continued_details["draft"]["delta"]["superseded"][0]["replacement_candidate_ids"][0],
        d.to_string()
    );
    let before_reads = versions(&pool, set).await;
    let delayed = client
        .call("save_candidate_set", initial_request.clone())
        .await;
    assert_eq!(delayed, initial);
    let history = read_collection_query(
        &mut client,
        &serde_json::json!({"route":"scope.candidates.context","params":json!({
            "candidate_set_id":set,"view":"history","limit":100
        })}),
    )
    .await;
    let history_items = &history.items;
    assert!(
        history_items
            .iter()
            .any(|item| item["history"]["candidate_id"] == c.to_string()
                && item["history"]["status"] == "superseded"
                && item["history"]["superseded_reason"] == "Export replaces audit in this request")
    );
    assert!(
        history_items
            .iter()
            .any(|item| item["history"]["candidate_id"] == b.to_string()
                && item["history"]["candidate_revision"] == 1
                && item["history"]["status"] == "prior")
    );
    for active in [a, d] {
        assert!(history_items.iter().any(|item| {
            item["history"]["candidate_id"] == active.to_string()
                && item["history"]["status"] == "active"
        }));
    }
    let mut after = 0;
    let mut historical_candidates = Vec::new();
    loop {
        let page = read_query_json(
            &mut client,
            &serde_json::json!({"route":"scope.candidates.context","params":json!({
                "candidate_set_id":set,"view":"historical","draft_revision":2,
                "after":after,"limit":2
            })}),
        )
        .await;
        assert_eq!(page.value["historical"]["set_revision"], 2);
        assert_eq!(
            page.value["historical"]["snapshot"]["id"],
            snapshot_one.to_string()
        );
        assert_eq!(page.value["historical"]["input_cursor"], 1);
        if after == 0 {
            assert_eq!(
                page.value["historical"]["snapshot"]["method"],
                created_overview["context"]["snapshot"]["method"]
            );
            assert_eq!(
                page.value["historical"]["snapshot"]["registry_digest"],
                created_overview["context"]["snapshot"]["registry_digest"]
            );
            assert_eq!(
                page.value["historical"]["snapshot"]["rules"],
                created_overview["context"]["snapshot"]["rules"]
            );
        }
        for item in page.value["items"].as_array().unwrap() {
            if let Some(candidate) = item.get("candidate") {
                historical_candidates.push(id(&candidate["id"]));
            }
        }
        let Some(next) = page.value["next_after"].as_i64() else {
            assert!(
                page.provenance
                    .terminal_actions
                    .iter()
                    .all(|action| action_name(action) != Some("scope.candidates.refresh"))
            );
            break;
        };
        let next_action = &page.provenance.terminal_actions[0];
        assert_eq!(action_params(next_action)["draft_revision"], 2);
        assert_eq!(action_params(next_action)["after"], next);
        assert!(next > after);
        after = next;
    }
    historical_candidates.sort();
    let mut expected = vec![a, b, c];
    expected.sort();
    assert_eq!(historical_candidates, expected);
    assert_eq!(
        read_text(&mut client, set, source_one, Some(2)).await,
        original_request
    );
    let mismatch = client
        .call_error(
            "candidate_context",
            json!({
                "candidate_set_id":set,"view":"fragment","draft_revision":2,
                "source_ref_id":current_two,"cursor":0
            }),
        )
        .await;
    assert_eq!(mismatch["error"]["code"], "not_found");
    assert_eq!(versions(&pool, set).await, before_reads);
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let mut resumed = Mcp::start(&socket, &config, &native, &workspace).await;
    let state = resumed.call("get_state", json!({})).await;
    assert_eq!(state["candidate_sets"][0]["id"], set.to_string());
    let restored = read_collection_query(
        &mut resumed,
        &serde_json::json!({"route":"scope.candidates.context","params":json!({
            "candidate_set_id":set,"view":"candidates","limit":100
        })}),
    )
    .await;
    let restored_ids = restored
        .items
        .iter()
        .filter_map(|item| item.get("candidate"))
        .map(|value| id(&value["id"]))
        .collect::<Vec<_>>();
    assert_eq!(
        restored.pages[0].value["context"]["candidate_set"]["revision"],
        6
    );
    assert!(restored_ids.contains(&a) && restored_ids.contains(&b) && restored_ids.contains(&d));
    assert!(!restored_ids.contains(&c));
    let restored_b = restored
        .items
        .iter()
        .filter_map(|item| item.get("candidate"))
        .find(|candidate| candidate["id"] == b.to_string())
        .unwrap();
    assert_eq!(restored_b["revision"], 2);
    assert_eq!(
        restored.pages[0].value["context"]["candidate_set"]["latest_input"],
        2
    );
    let restored_inputs = read_collection_query(&mut resumed, &serde_json::json!({"route":"scope.candidates.context","params":json!({"candidate_set_id":set,"view":"inputs","limit":25})})).await;
    let input_items = &restored_inputs.items;
    assert_eq!(input_items.len(), 2);
    assert_eq!(
        read_text(
            &mut resumed,
            set,
            id(&input_items[0]["input"]["source_ref_id"]),
            None,
        )
        .await,
        original_request
    );
    assert_eq!(
        read_text(
            &mut resumed,
            set,
            id(&input_items[1]["input"]["source_ref_id"]),
            None,
        )
        .await,
        amendment
    );
    let decisions = [a, b, d]
        .map(|candidate_id| {
            json!({
                "candidate_id":candidate_id,"decision":"accept",
                "rationale":"Bounded amendment result"
            })
        })
        .to_vec();
    let ready = resumed.call("save_candidate_set", json!({
        "kind":"review","candidate_set_id":set,"revision":6,
        "snapshot_id":current["snapshot"]["id"],"input_cursor":2,"request_id":Uuid::new_v4(),
        "review":{"verdict":"ready","summary":"The amendment delta is explicit and each current result is bounded.",
        "findings":[],"candidate_decisions":decisions}
    })).await;
    let ready_fixture = CandidateFixture::from_mutation(ready.clone());
    let ready_overview = ready_fixture.read_overview(&mut resumed).await.value;
    let ready_reviews =
        recovery_support::candidate_reviews::read_reviews(&ready_fixture, &mut resumed).await;
    assert_eq!(
        ready_reviews.exact(ready["candidate_set"]["revision"].as_i64().unwrap())["verdict"],
        "ready"
    );
    assert_eq!(ready_reviews.latest()["verdict"], "ready");
    assert_eq!(
        ready_overview["context"]["candidate_set"]["status"],
        "ready"
    );
    resumed.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
