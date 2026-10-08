//! A completed native cycle can receive distinct new work through its advertised MCP path.
//! Uses only explicitly configured fixture PostgreSQL and a private fixture daemon.
#[allow(dead_code)]
#[path = "pipeline_execution/lifecycle_support.rs"]
mod lifecycle_support;
#[allow(dead_code)]
mod recovery_support;
#[allow(dead_code)]
#[path = "native_planning/support.rs"]
mod support;

use lifecycle_support::{LIGHTWEIGHT_PHASES, complete, lightweight_draft, terminal};
use recovery_support::native_reads::{ScopeOpenFixture, SlicePlanningFixture};
use recovery_support::pipeline_reads::resolve_pipeline;
use recovery_support::{
    Daemon, Mcp, action_name, action_params, host_file, private_temp, tagged_url,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{id, open_slice, ready_source_candidate, repository, review, route, save};
use tect_postgres::admin;
use uuid::Uuid;

fn existing_work(node: &Value) -> Value {
    json!({"kind":"work",
        "identity":{"candidate_id":node["id"],"revision":node["revision"]},
        "title":node["title"],"outcome":node["outcome"],"includes":node["includes"],
        "excludes":node["excludes"],"dependencies":[],"proof":node["proof"],
        "pipeline":node["pipeline"],"pipeline_reason":node["pipeline_reason"],
        "source_result_ids":node["source_result_ids"]})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn completed_cycle_offers_new_input_and_opens_distinct_successor_without_reopening_history() {
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
    let socket = root.join("native-continuation.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-native-continuation-{}", Uuid::new_v4()),
    );
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace = format!("native-continuation-{}", Uuid::new_v4());
    let mut client = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &workspace).await;
    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;
    let opened_scope = route(
        &mut client,
        "command",
        "scope.open",
        json!({
            "request_id":Uuid::new_v4(),"candidate_set_id":source["candidate_set"]["id"],
            "candidate_set_revision":source["candidate_set"]["revision"],
            "candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],
            "candidate_revision":candidate["revision"]
        }),
    )
    .await;
    let opened_scope = ScopeOpenFixture::from_mutation(opened_scope, "created");
    let planning = opened_scope.read_planning(&mut client).await.value;
    let saved = save(&mut client, &planning, lightweight_draft()).await;
    let reviewed = review(&mut client, &saved).await;
    let original_node = reviewed["draft"]["nodes"][0].clone();
    let opened_slice = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&reviewed, &original_node, Uuid::new_v4()),
    )
    .await;
    let original_slice = opened_slice["created"].clone();
    let begun = route(&mut client, "command", "slice.pipeline.begin", json!({
        "request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
        "slice_id":original_slice["id"],"slice_revision":original_slice["revision"],
        "delivery_mode":"whole","qualification_reason":"Inspect the complete bounded fixture contract."
    })).await;
    let mut pipeline = resolve_pipeline(&mut client, begun).await.unwrap();
    for (index, phase) in LIGHTWEIGHT_PHASES.iter().enumerate() {
        assert_eq!(pipeline.run()["current_phase_id"], *phase);
        let final_phase = index + 1 == LIGHTWEIGHT_PHASES.len();
        pipeline = complete(
            &mut client,
            &pipeline,
            "completed",
            if final_phase { "complete" } else { "continue" },
            final_phase.then(|| terminal("The original structural fixture cycle is complete.")),
            false,
        )
        .await
        .0;
    }
    assert_eq!(pipeline.run()["status"], "completed");
    let historical_run = pipeline.run().clone();
    let historical_result = pipeline.details_data()["result"].clone();
    assert_eq!(
        historical_result["pipeline_result_origin"],
        "managed_completed"
    );
    assert_eq!(historical_result["slice_id"], original_slice["id"]);

    let reopened_workspace = client.call("open_workspace", json!({})).await;
    let actions = reopened_workspace["actions"].as_array().unwrap();
    let input_index = actions
        .iter()
        .position(|a| {
            action_name(a) == Some("slice.candidates.input")
                && action_params(a)["scope_id"] == reviewed["scope"]["id"]
        })
        .expect("completed cycle must advertise explicit new-work input");
    let input_action = &actions[input_index];
    assert_eq!(input_action["kind"], "needs_input");
    assert_eq!(input_action["tool"], "command");
    assert_eq!(
        action_params(input_action)["candidate_set_id"],
        reviewed["candidate_set"]["id"]
    );
    assert!(
        action_params(input_action)["revision"].as_i64().unwrap()
            > reviewed["candidate_set"]["revision"].as_i64().unwrap()
    );
    assert!(
        action_params(input_action).get("input").is_none(),
        "completed input must never be offered as fresh authorized work"
    );
    let historical_action_index = actions
        .iter()
        .position(|a| {
            action_name(a) == Some("slice.pipeline.context")
                && action_params(a)["run_id"] == historical_run["id"]
        })
        .unwrap();
    assert!(
        input_index < historical_action_index,
        "new work must precede completed history"
    );
    assert!(!actions.iter().any(|a| action_name(a) == Some("slice.open")
        && action_params(a)["candidate_id"] == original_node["id"]));

    let fresh_input = "Add a distinct bounded correction to preserve the completed preview result and cover a newly authorized edge case.";
    let mut input_params = action_params(input_action).clone();
    input_params["input"] = json!(fresh_input);
    // Follow the exact offered route and pinned revision, supplying only new caller input.
    let input_mutation = client
        .call(
            input_action["tool"].as_str().unwrap(),
            json!({
                "route":action_name(input_action).unwrap(),"params":input_params
            }),
        )
        .await;
    let input_context = SlicePlanningFixture::from_mutation(input_mutation)
        .read_details(&mut client)
        .await
        .value;
    assert!(
        input_context["stale_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "planning_inputs")
    );
    let latest_input: (Option<Uuid>, String) = sqlx::query_as(
        "SELECT source_result_id,input FROM slice_planning_inputs WHERE candidate_set_id=$1 ORDER BY sequence DESC LIMIT 1")
        .bind(id(&input_context["candidate_set"]["id"])).fetch_one(&pool).await.unwrap();
    assert_eq!(
        latest_input,
        (None, fresh_input.to_owned()),
        "new caller input must be distinct from the prior managed Result input"
    );
    let refreshed = route(&mut client, "command", "slice.candidates.refresh", json!({
        "scope_id":input_context["scope"]["id"],"candidate_set_id":input_context["candidate_set"]["id"],
        "revision":input_context["candidate_set"]["revision"],"request_id":Uuid::new_v4()
    })).await;
    let refreshed = SlicePlanningFixture::from_mutation(refreshed)
        .read_details(&mut client)
        .await
        .value;
    assert!(refreshed["stale_reasons"].as_array().unwrap().is_empty());
    assert!(
        refreshed["snapshot"]["result_ids"]
            .as_array()
            .unwrap()
            .contains(&historical_result["id"])
    );
    let continued = save(&mut client, &refreshed, json!({
        "coverage_summary":"Preserve completed work and add only the newly authorized edge case",
        "nodes":[existing_work(&original_node),{
            "kind":"work","identity":{"local":"successor"},"title":"Correct the newly authorized edge case",
            "outcome":"The distinct edge case has focused regression coverage",
            "includes":["new edge case"],"excludes":["completed behavior rewrite","deployment"],
            "dependencies":[{"candidate_id":original_node["id"],"revision":original_node["revision"]}],
            "proof":["Focused edge-case regression passes"],"pipeline":"slice.lightweight-tdd-development",
            "pipeline_reason":"New input requests a distinct bounded correction",
            "source_result_ids":[historical_result["id"]]
        }],"supersessions":[]
    })).await;
    let nodes = continued["draft"]["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 2);
    assert_eq!(
        nodes
            .iter()
            .find(|node| node["id"] == original_node["id"])
            .unwrap(),
        &original_node,
        "opened node must survive unchanged"
    );
    let successor = nodes
        .iter()
        .find(|node| node["id"] != original_node["id"])
        .unwrap()
        .clone();
    let ready = review(&mut client, &continued).await;
    let successor_open = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&ready, &successor, Uuid::new_v4()),
    )
    .await;
    assert_ne!(successor_open["created"]["id"], original_slice["id"]);
    assert_eq!(successor_open["created"]["candidate_id"], successor["id"]);
    assert_eq!(successor_open["created"]["pipeline_status"], "not_started");

    let historical_read = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":historical_run["id"]}),
    )
    .await;
    let historical_read = resolve_pipeline(&mut client, historical_read)
        .await
        .unwrap();
    assert_eq!(
        historical_read.run(),
        &historical_run,
        "completed pipeline must remain unchanged"
    );
    assert_eq!(historical_read.details_data()["result"], historical_result);
    let slices: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT candidate_id,state FROM native_slices WHERE scope_id=$1 ORDER BY candidate_id",
    )
    .bind(id(&ready["scope"]["id"]))
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        slices.len(),
        2,
        "one historical and one successor Slice only"
    );
    assert!(slices.contains(&(id(&original_node["id"]), "completed".to_owned())));
    assert!(slices.contains(&(id(&successor["id"]), "open".to_owned())));
    client.finish().await;
}
