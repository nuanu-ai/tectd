mod recovery_support;
#[path = "native_planning/support.rs"]
mod support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{
    id, open_slice, ready_source_candidate, repository, review, route, route_error, save,
};
use tect_postgres::admin;
use uuid::Uuid;

fn initial_draft() -> Value {
    json!({"coverage_summary":"Diagnosis followed by explicit decision","nodes":[
    {"kind":"work","identity":{"local":"debug"},"title":"Demonstrate preview cause",
     "outcome":"Cause and correction direction are demonstrated","includes":["reproduction","cause"],
     "excludes":["fix","deployment"],"dependencies":[],"proof":["Causal observation"],
     "pipeline":"slice.debug-root-cause","pipeline_reason":"Cause is unknown","source_result_ids":[]},
    {"kind":"decision","identity":{"local":"decision"},"title":"Choose correction path",
     "question":"Is Lightweight sufficient or is justified Full required?",
     "resolution_criteria":["Diagnosis identifies affected boundary"],
     "dependencies":[{"local":"debug"}],"source_result_ids":[]}
],"supersessions":[]})
}

fn existing_work(node: &Value) -> Value {
    json!({"kind":"work",
    "identity":{"candidate_id":node["id"],"revision":node["revision"]},"title":node["title"],
    "outcome":node["outcome"],"includes":node["includes"],"excludes":node["excludes"],
    "dependencies":[],"proof":node["proof"],"pipeline":node["pipeline"],
    "pipeline_reason":node["pipeline_reason"],"source_result_ids":node["source_result_ids"]})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn native_scope_slice_result_replans_and_recovers() {
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
    let socket = root.join("native-slices.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-native-slices-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let key = format!("native-slices-{}", Uuid::new_v4());
    let mut client = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &key).await;
    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;

    let pipelines = route(&mut client, "query", "slice.pipelines", json!({})).await;
    assert_eq!(pipelines["pipelines"].as_array().unwrap().len(), 7);
    assert_eq!(pipelines["executable"], true);
    assert_eq!(pipelines["executable_count"], 7);
    assert!(
        !pipelines
            .to_string()
            .contains("slice.hybrid-implementation-operation")
    );
    let scope_request = json!({"request_id":Uuid::new_v4(),"candidate_set_id":source["candidate_set"]["id"],
        "candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],
        "candidate_revision":candidate["revision"]});
    let opened = route(&mut client, "command", "scope.open", scope_request.clone()).await;
    let created = &opened["created"];
    let replay = route(&mut client, "command", "scope.open", scope_request).await;
    assert_eq!(replay["replay"], *created);
    assert_eq!(created["planning"]["snapshot"]["sequence"], 1);
    assert_eq!(
        created["planning"]["snapshot"]["rules"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        created["planning"]["snapshot"]["catalogue"]["entries"]
            .as_array()
            .unwrap()
            .len(),
        7
    );

    let planning = &created["planning"];
    let malformed = json!({"coverage_summary":"Invalid Full","nodes":[{"kind":"work",
        "identity":{"local":"full"},"title":"Full","outcome":"Large change","proof":["proof"],
        "pipeline":"slice.full-design-to-execution","pipeline_reason":"Large"}]});
    let mut bad_params = json!({"kind":"draft","scope_id":planning["scope"]["id"],
        "candidate_set_id":planning["candidate_set"]["id"],"revision":planning["candidate_set"]["revision"],
        "snapshot_id":planning["snapshot"]["id"],"input_cursor":planning["candidate_set"]["input_cursor"],
        "request_id":Uuid::new_v4(),"draft":malformed});
    let full_error = route_error(
        &mut client,
        "command",
        "slice.candidates.save",
        bad_params.clone(),
    )
    .await;
    assert_eq!(full_error["error"]["code"], "invalid_arguments");
    bad_params["draft"]["nodes"][0]["pipeline"] = json!("slice.hybrid-implementation-operation");
    bad_params["request_id"] = json!(Uuid::new_v4());
    let hybrid_error =
        route_error(&mut client, "command", "slice.candidates.save", bad_params).await;
    assert_eq!(hybrid_error["error"]["code"], "invalid_arguments");

    let saved = save(&mut client, planning, initial_draft()).await;
    let debug = saved["draft"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "work")
        .unwrap()
        .clone();
    let decision = saved["draft"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "decision")
        .unwrap()
        .clone();
    let reviewed = review(&mut client, &saved).await;
    assert_eq!(reviewed["candidate_set"]["status"], "ready");
    let decision_error = route_error(
        &mut client,
        "command",
        "slice.open",
        open_slice(&reviewed, &decision, Uuid::new_v4()),
    )
    .await;
    assert_eq!(decision_error["error"]["code"], "forbidden");
    let open_request = open_slice(&reviewed, &debug, Uuid::new_v4());
    let slice_opened = route(&mut client, "command", "slice.open", open_request.clone()).await;
    let slice = &slice_opened["created"];
    assert_eq!(slice["pipeline_status"], "not_started");
    assert_eq!(slice["execution_claimed"], false);
    assert!(
        slice_opened["actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["tool"] != "execute")
    );

    let result_request = json!({"request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
        "slice_id":slice["id"],"slice_revision":slice["revision"],"outcome":"completed",
        "summary":"Caller reports bounded diagnosis complete","evidence":[{"kind":"test",
        "reference":"native fixture","observation":"Result persisted"}],
        "scope_impact":"Decision can be resolved","remaining_work":"Refresh and review correction"});
    let recorded = route(
        &mut client,
        "command",
        "slice.result.record",
        result_request.clone(),
    )
    .await;
    let result = &recorded["created"]["result"];
    let stale = &recorded["created"]["context"];
    assert_eq!(result["provenance"], "externally_reported");
    assert!(
        stale["stale_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r == "planning_inputs")
    );
    let replay_result = route(
        &mut client,
        "command",
        "slice.result.record",
        result_request.clone(),
    )
    .await;
    assert_eq!(replay_result["replay"]["result"], *result);
    let mut changed = result_request;
    changed["summary"] = json!("changed");
    let conflict = route_error(&mut client, "command", "slice.result.record", changed).await;
    assert_eq!(conflict["error"]["code"], "input_conflict");
    let replay_slice = route(&mut client, "command", "slice.open", open_request).await;
    assert_eq!(replay_slice["replay"], *slice);

    let stale_open = route_error(
        &mut client,
        "command",
        "slice.open",
        open_slice(&reviewed, &decision, Uuid::new_v4()),
    )
    .await;
    assert!(matches!(
        stale_open["error"]["code"].as_str(),
        Some("stale_context" | "stale_revision")
    ));
    let refreshed = route(
        &mut client,
        "command",
        "slice.candidates.refresh",
        json!({
        "scope_id":stale["scope"]["id"],"candidate_set_id":stale["candidate_set"]["id"],
        "revision":stale["candidate_set"]["revision"],"request_id":Uuid::new_v4()}),
    )
    .await;
    assert!(refreshed["stale_reasons"].as_array().unwrap().is_empty());
    assert!(
        refreshed["snapshot"]["result_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == &result["id"])
    );

    let changed_opened = json!({"coverage_summary":"Illegal rewrite","nodes":[{
        "kind":"work","identity":{"candidate_id":debug["id"],"revision":debug["revision"]},
        "change_rationale":"rewrite","title":debug["title"],"outcome":"Changed opened outcome",
        "proof":debug["proof"],"pipeline":debug["pipeline"],"pipeline_reason":debug["pipeline_reason"]}]});
    let mut rewrite = json!({"kind":"draft","scope_id":refreshed["scope"]["id"],
        "candidate_set_id":refreshed["candidate_set"]["id"],"revision":refreshed["candidate_set"]["revision"],
        "snapshot_id":refreshed["snapshot"]["id"],"input_cursor":refreshed["candidate_set"]["input_cursor"],
        "request_id":Uuid::new_v4(),"draft":changed_opened});
    let protected = route_error(
        &mut client,
        "command",
        "slice.candidates.save",
        rewrite.clone(),
    )
    .await;
    assert_eq!(protected["error"]["code"], "forbidden");

    rewrite["request_id"] = json!(Uuid::new_v4());
    rewrite["draft"] = json!({
        "coverage_summary":"Result resolves decision to bounded correction","nodes":[existing_work(&debug),{
            "kind":"work","identity":{"local":"correction"},"title":"Correct preview derivation",
            "outcome":"Preview reflects saved settings","includes":["correction"],"excludes":["redesign"],
            "dependencies":[{"candidate_id":debug["id"],"revision":debug["revision"]}],
            "proof":["Focused regression passes"],"pipeline":"slice.lightweight-tdd-development",
            "pipeline_reason":"Result isolates a small correction","source_result_ids":[result["id"]]
        }],"supersessions":[{"candidate_id":decision["id"],"revision":decision["revision"],
            "reason":"Result resolves decision","replacements":[{"local":"correction"}]}]});
    let revised = route(&mut client, "command", "slice.candidates.save", rewrite).await;
    let light = revised["draft"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["pipeline"] == "slice.lightweight-tdd-development")
        .unwrap()
        .clone();
    let final_plan = review(&mut client, &revised).await;
    let follow = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&final_plan, &light, Uuid::new_v4()),
    )
    .await;
    assert_eq!(
        follow["created"]["pipeline"],
        "slice.lightweight-tdd-development"
    );

    let scope_id = id(&created["scope"]["id"]);
    let slice_id = id(&slice["id"]);
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let mut restored = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &key).await;
    restored.call("open_workspace", json!({})).await;
    let scope = route(
        &mut restored,
        "query",
        "scope.context",
        json!({"scope_id":scope_id}),
    )
    .await;
    let old_slice = route(
        &mut restored,
        "query",
        "slice.context",
        json!({"slice_id":slice_id}),
    )
    .await;
    assert_eq!(scope["id"], scope_id.to_string());
    assert_eq!(old_slice["state"], "completed");
    restored.finish().await;
}
