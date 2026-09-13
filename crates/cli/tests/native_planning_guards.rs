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

fn scope_request(source: &Value, candidate: &Value) -> Value {
    json!({
        "request_id":Uuid::new_v4(),
        "candidate_set_id":source["candidate_set"]["id"],
        "candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],
        "candidate_id":candidate["id"],
        "candidate_revision":candidate["revision"]
    })
}

fn work(local: &str, dependencies: Vec<Value>) -> Value {
    json!({
        "kind":"work","identity":{"local":local},"title":format!("Work {local}"),
        "outcome":format!("Outcome {local} is demonstrated"),"includes":[local],"excludes":[],
        "dependencies":dependencies,"proof":[format!("Proof {local}")],
        "pipeline":"slice.lightweight-tdd-development",
        "pipeline_reason":"Bounded behavior with a focused test cycle","source_result_ids":[]
    })
}

fn existing_work(node: &Value, dependencies: Vec<Value>) -> Value {
    json!({
        "kind":"work","identity":{"candidate_id":node["id"],"revision":node["revision"]},
        "title":node["title"],"outcome":node["outcome"],"includes":node["includes"],
        "excludes":node["excludes"],"dependencies":dependencies,"proof":node["proof"],
        "pipeline":node["pipeline"],"pipeline_reason":node["pipeline_reason"],
        "source_result_ids":node["source_result_ids"]
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn native_planning_rejects_cross_boundary_and_unready_graph_operations() {
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
    let socket = root.join("native-guards.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-native-guards-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace = format!("native-guards-{}", Uuid::new_v4());
    let mut client = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &workspace).await;
    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;

    sqlx::query("UPDATE scope_candidate_sets SET status='review_required' WHERE id=$1")
        .bind(id(&source["candidate_set"]["id"]))
        .execute(&pool)
        .await
        .unwrap();
    let unready = route_error(
        &mut client,
        "command",
        "scope.open",
        scope_request(&source, &candidate),
    )
    .await;
    assert_eq!(unready["error"]["code"], "forbidden");
    sqlx::query("UPDATE scope_candidate_sets SET status='ready' WHERE id=$1")
        .bind(id(&source["candidate_set"]["id"]))
        .execute(&pool)
        .await
        .unwrap();

    let opened = route(
        &mut client,
        "command",
        "scope.open",
        scope_request(&source, &candidate),
    )
    .await;
    let planning = &opened["created"]["planning"];

    let cycle = json!({"coverage_summary":"Cycle must fail","nodes":[
        work("a",vec![json!({"local":"b"})]),work("b",vec![json!({"local":"a"})])
    ],"supersessions":[]});
    let cycle_error = route_error(
        &mut client,
        "command",
        "slice.candidates.save",
        json!({"kind":"draft","scope_id":planning["scope"]["id"],
            "candidate_set_id":planning["candidate_set"]["id"],
            "revision":planning["candidate_set"]["revision"],"snapshot_id":planning["snapshot"]["id"],
            "input_cursor":planning["candidate_set"]["input_cursor"],"request_id":Uuid::new_v4(),
            "draft":cycle}),
    )
    .await;
    assert_eq!(cycle_error["error"]["code"], "invalid_arguments");

    let dangling = json!({"coverage_summary":"Dangling dependency must fail","nodes":[
        work("a",vec![json!({"local":"missing"})])
    ],"supersessions":[]});
    let dangling_error = route_error(
        &mut client,
        "command",
        "slice.candidates.save",
        json!({"kind":"draft","scope_id":planning["scope"]["id"],
            "candidate_set_id":planning["candidate_set"]["id"],
            "revision":planning["candidate_set"]["revision"],"snapshot_id":planning["snapshot"]["id"],
            "input_cursor":planning["candidate_set"]["input_cursor"],"request_id":Uuid::new_v4(),
            "draft":dangling}),
    )
    .await;
    assert_eq!(dangling_error["error"]["code"], "invalid_arguments");

    let draft = json!({"coverage_summary":"Ordered work and result decision","nodes":[
        work("a",vec![]),work("b",vec![json!({"local":"a"})]),
        {"kind":"decision","identity":{"local":"decision"},"title":"Choose successor",
         "question":"Does the result require Full?","resolution_criteria":["Result bounds complexity"],
         "dependencies":[{"local":"a"}],"source_result_ids":[]},
        {"kind":"decision","identity":{"local":"optional"},"title":"Check optional follow-up",
         "question":"Is optional follow-up still required?","resolution_criteria":["Result covers it"],
         "dependencies":[{"local":"a"}],"source_result_ids":[]}
    ],"supersessions":[]});
    let saved = save(&mut client, planning, draft).await;
    let a = saved["draft"]["nodes"][0].clone();
    let b = saved["draft"]["nodes"][1].clone();
    let decision = saved["draft"]["nodes"][2].clone();
    let optional = saved["draft"]["nodes"][3].clone();
    let ready = review(&mut client, &saved).await;
    let unfinished = route_error(
        &mut client,
        "command",
        "slice.open",
        open_slice(&ready, &b, Uuid::new_v4()),
    )
    .await;
    assert_eq!(unfinished["error"]["code"], "forbidden");
    let opened_a = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&ready, &a, Uuid::new_v4()),
    )
    .await;
    let slice_a = &opened_a["created"];
    let recorded = route(
        &mut client,
        "command",
        "slice.result.record",
        json!({"request_id":Uuid::new_v4(),"scope_id":ready["scope"]["id"],
            "slice_id":slice_a["id"],"slice_revision":slice_a["revision"],"outcome":"completed",
            "summary":"External meows; evidence reports intertwined boundaries","evidence":[{
                "kind":"test","reference":"guard fixture","observation":"Coupling requires one vertical Full successor"}],
            "scope_impact":"Decision has evidence","remaining_work":"Refresh and resolve the decision"}),
    )
    .await;
    let result = &recorded["created"]["result"];
    let stale = &recorded["created"]["context"];
    let refreshed = route(
        &mut client,
        "command",
        "slice.candidates.refresh",
        json!({"scope_id":stale["scope"]["id"],"candidate_set_id":stale["candidate_set"]["id"],
            "revision":stale["candidate_set"]["revision"],"request_id":Uuid::new_v4()}),
    )
    .await;

    let missing_full = json!({"coverage_summary":"Missing Full rationale","nodes":[
        existing_work(&a,vec![]),existing_work(&b,vec![json!({"candidate_id":a["id"],"revision":a["revision"]})]),{"kind":"work","identity":{"local":"full"},
        "title":"Full successor","outcome":"Intertwined correction is verified",
        "includes":["coherent correction"],"excludes":["deployment"],"dependencies":[{
            "candidate_id":a["id"],"revision":a["revision"]}],"proof":["Integration proof"],
        "pipeline":"slice.full-design-to-execution","pipeline_reason":"Result demonstrates coupling",
        "source_result_ids":[result["id"]]}],"supersessions":[{"candidate_id":decision["id"],
        "revision":decision["revision"],"reason":"Result selects Full","source_result_ids":[result["id"]],
        "replacements":[{"local":"full"}]},{"candidate_id":optional["id"],"revision":optional["revision"],
        "reason":"Result proves no optional follow-up is needed","source_result_ids":[result["id"]],"replacements":[]}]});
    let missing_full_error = route_error(
        &mut client,
        "command",
        "slice.candidates.save",
        json!({"kind":"draft","scope_id":refreshed["scope"]["id"],
            "candidate_set_id":refreshed["candidate_set"]["id"],
            "revision":refreshed["candidate_set"]["revision"],"snapshot_id":refreshed["snapshot"]["id"],
            "input_cursor":refreshed["candidate_set"]["input_cursor"],"request_id":Uuid::new_v4(),
            "draft":missing_full}),
    )
    .await;
    assert_eq!(missing_full_error["error"]["code"], "invalid_arguments");

    let justified_full = json!({"coverage_summary":"Result justifies Full successor","nodes":[
        existing_work(&a,vec![]),existing_work(&b,vec![json!({"candidate_id":a["id"],"revision":a["revision"]})]),{"kind":"work","identity":{"local":"full"},
        "title":"Full successor","outcome":"Intertwined correction is verified",
        "includes":["coherent correction"],"excludes":["deployment"],"dependencies":[{
            "candidate_id":a["id"],"revision":a["revision"]}],"proof":["Integration proof"],
        "pipeline":"slice.full-design-to-execution","pipeline_reason":"Result demonstrates coupling",
        "why_lightweight_insufficient":"The reported boundaries must change coherently",
        "why_further_vertical_split_not_viable":"Splitting would leave no observable working outcome",
        "source_result_ids":[result["id"]]}],"supersessions":[{"candidate_id":decision["id"],
        "revision":decision["revision"],"reason":"Result selects justified Full","source_result_ids":[result["id"]],
        "replacements":[{"local":"full"}]},{"candidate_id":optional["id"],"revision":optional["revision"],
        "reason":"Result proves no optional follow-up is needed","source_result_ids":[result["id"]],"replacements":[]}]});
    let mut unbacked_no_work = justified_full.clone();
    unbacked_no_work["supersessions"][1]["source_result_ids"] = json!([]);
    let unbacked_error = route_error(
        &mut client,
        "command",
        "slice.candidates.save",
        json!({"kind":"draft","scope_id":refreshed["scope"]["id"],
            "candidate_set_id":refreshed["candidate_set"]["id"],
            "revision":refreshed["candidate_set"]["revision"],"snapshot_id":refreshed["snapshot"]["id"],
            "input_cursor":refreshed["candidate_set"]["input_cursor"],"request_id":Uuid::new_v4(),
            "draft":unbacked_no_work}),
    )
    .await;
    assert_eq!(unbacked_error["error"]["code"], "invalid_arguments");
    let revised = save(&mut client, &refreshed, justified_full).await;
    assert!(
        revised["draft"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| {
                node["pipeline"] == "slice.full-design-to-execution"
                    && node["source_result_ids"][0] == result["id"]
            })
    );
    assert!(
        revised["draft"]["supersessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["candidate_id"] == optional["id"]
                && entry["source_result_ids"][0] == result["id"]
                && entry["replacement_candidate_ids"]
                    .as_array()
                    .unwrap()
                    .is_empty())
    );

    let other_workspace = format!("native-guards-other-{}", Uuid::new_v4());
    let mut other = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &other_workspace,
    )
    .await;
    other.call("open_workspace", json!({})).await;
    let isolated = route_error(
        &mut other,
        "query",
        "scope.context",
        json!({"scope_id":ready["scope"]["id"]}),
    )
    .await;
    assert!(matches!(
        isolated["error"]["code"].as_str(),
        Some("not_found" | "forbidden")
    ));

    let (source_two, candidate_two) = ready_source_candidate(&mut client, &repo).await;
    let opened_two = route(
        &mut client,
        "command",
        "scope.open",
        scope_request(&source_two, &candidate_two),
    )
    .await;
    let planning_two = &opened_two["created"]["planning"];
    let cross_result = json!({"coverage_summary":"Cross Scope result must fail","nodes":[{
        "kind":"work","identity":{"local":"cross"},"title":"Cross","outcome":"Cross",
        "dependencies":[],"proof":["proof"],"pipeline":"slice.lightweight-tdd-development",
        "pipeline_reason":"bounded","source_result_ids":[result["id"]]}],"supersessions":[]});
    let cross_error = route_error(
        &mut client,
        "command",
        "slice.candidates.save",
        json!({"kind":"draft","scope_id":planning_two["scope"]["id"],
            "candidate_set_id":planning_two["candidate_set"]["id"],
            "revision":planning_two["candidate_set"]["revision"],"snapshot_id":planning_two["snapshot"]["id"],
            "input_cursor":planning_two["candidate_set"]["input_cursor"],"request_id":Uuid::new_v4(),
            "draft":cross_result}),
    )
    .await;
    assert_eq!(cross_error["error"]["code"], "invalid_arguments");

    other.finish().await;
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
